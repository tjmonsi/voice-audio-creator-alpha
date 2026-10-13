"""narrate worker: model work and pinned Hugging Face downloads for the narrate CLI.

Protocol 1: one JSON object per line on stdin (requests) and on a private copy of the
original stdout (responses). Every other output goes to stderr. See
specs/spec-narrate-worker.md. Started as: python -I -X utf8 -u narrate_worker.py
"""

import os
import sys

# [FR-005] Requirement 1: keep a private handle to the real stdout before any third-party
# import, then point fd 1 and sys.stdout at stderr so library prints cannot corrupt the channel.
_channel = open(os.dup(1), "w", encoding="utf-8", newline="\n", buffering=1)
os.dup2(2, 1)
sys.stdout = sys.stderr

import json  # noqa: E402
import queue  # noqa: E402
import signal  # noqa: E402
import threading  # noqa: E402
import time  # noqa: E402
import traceback  # noqa: E402

PROTOCOL = 1
WORKER_NAME = "narrate-worker 0.1.0"
SAMPLE_RATE = 24000
KOKORO_REPO = "hexgrad/Kokoro-82M"


class OpError(Exception):
    """A request failure that maps to one protocol error code."""

    def __init__(self, code, message, with_traceback=False):
        super().__init__(message)
        self.code = code
        self.message = message
        self.with_traceback = with_traceback


def log(message):
    print(f"narrate-worker: {message}", file=sys.stderr, flush=True)


def send(obj):
    _channel.write(json.dumps(obj, separators=(",", ":")) + "\n")
    _channel.flush()


def error_reply(request_id, code, message, tb=None):
    return {
        "id": request_id,
        "ok": False,
        "error": {"code": code, "message": message.splitlines()[0] if message else code, "traceback": tb},
    }


def is_oom(exc):
    name = type(exc).__name__
    return name == "OutOfMemoryError" or (isinstance(exc, RuntimeError) and "out of memory" in str(exc).lower())


def to_hex(tensor_or_array):
    import numpy as np

    if hasattr(tensor_or_array, "detach"):
        tensor_or_array = tensor_or_array.detach().cpu().float().numpy()
    samples = np.ascontiguousarray(np.asarray(tensor_or_array).reshape(-1), dtype="<f4")
    return samples.size, samples.tobytes().hex()


def release_cuda():
    import gc

    gc.collect()
    try:
        import torch

        if torch.cuda.is_available():
            # Emptying the allocator cache while cuFFT plans still hold freed workspace made the next
            # torch.istft call (Kokoro) stall on torch 2.6.0+cu124; drop the plans first.
            torch.backends.cuda.cufft_plan_cache.clear()
            torch.cuda.empty_cache()
    except Exception:  # best effort cleanup; never fails a request
        pass


class Session:
    """State of one worker process: the loaded model and one conditioning per voice."""

    def __init__(self):
        self.model = None
        self.voices = {}

    # -- ops -------------------------------------------------------------------------------

    def op_hello(self, req):
        if req.get("protocol") != PROTOCOL:
            raise OpError("bad_request", f"unsupported protocol {req.get('protocol')!r}")
        return {"protocol": PROTOCOL, "worker": WORKER_NAME, "python": ".".join(map(str, sys.version_info[:3]))}

    def op_probe(self, req):
        try:
            import torch

            available = bool(torch.cuda.is_available())
            name = torch.cuda.get_device_name(0) if available else None
            return {"cuda_available": available, "device_name": name, "torch": str(torch.__version__)}
        except Exception as exc:
            raise OpError("probe_failed", f"probe failed: {exc}", True) from exc

    def resolve_device(self, requested, torch, unavailable_code="cuda_unavailable"):
        available = bool(torch.cuda.is_available())
        if requested == "cpu":
            return "cpu", False
        if requested == "cuda":
            if not available:
                raise OpError(unavailable_code, "CUDA is not available")
            return "cuda", False
        if requested == "auto":
            return ("cuda", False) if available else ("cpu", True)
        raise OpError("bad_request", f"unknown device {requested!r}")

    def op_load(self, req):
        started = time.monotonic()
        model_dir = require(req, "model_dir", str)
        requested = require(req, "device", str)

        # [FR-018] Requirement 4: never produce unwatermarked audio.
        if os.environ.get("NARRATE_TEST_WATERMARK_FAIL") == "1":
            raise OpError("watermarker_unavailable", "watermarker disabled by NARRATE_TEST_WATERMARK_FAIL")
        try:
            import perth
        except Exception as exc:
            raise OpError("watermarker_unavailable", f"cannot import perth: {exc}", True) from exc
        if getattr(perth, "PerthImplicitWatermarker", None) is None:
            raise OpError("watermarker_unavailable", "perth.PerthImplicitWatermarker is unavailable (pkg_resources missing?)")

        import torch

        device, fallback = self.resolve_device(requested, torch)
        try:
            from chatterbox.tts_turbo import ChatterboxTurboTTS

            model = ChatterboxTurboTTS.from_local(model_dir, device)
        except Exception as exc:
            if is_oom(exc):
                release_cuda()
                raise OpError("out_of_memory", "out of memory while loading the model", True) from exc
            raise OpError("model_load_failed", f"{type(exc).__name__}: {exc}", True) from exc

        if getattr(model, "watermarker", None) is None:
            raise OpError("watermarker_unavailable", "the model has no watermarker")
        if model.sr != SAMPLE_RATE:
            raise OpError("model_load_failed", f"unexpected model sample rate {model.sr}")

        self.model = model
        self.voices = {}
        if model.conds is not None:
            # [FR-008] Requirement 4/5: keep the bundled voice before any clone replaces model.conds (research F7).
            self.voices["builtin"] = model.conds
        name = torch.cuda.get_device_name(0) if device == "cuda" else None
        return {"device": device, "device_name": name, "fallback": fallback, "load_seconds": time.monotonic() - started}

    def op_voice(self, req):
        voice_id = require(req, "voice_id", str)
        if self.model is None:
            raise OpError("bad_request", "voice requested before load")
        if voice_id == "builtin":
            if "builtin" not in self.voices:
                raise OpError("builtin_unavailable", "the model has no bundled conds.pt voice")
            return {"voice_id": voice_id}
        clip = req.get("clip")
        if not isinstance(clip, str) or not clip:
            raise OpError("bad_request", "voice needs a clip path")
        try:
            # [FR-007] Requirement 5: library defaults for exaggeration 0.0 and loudness normalization.
            self.model.prepare_conditionals(clip, exaggeration=0.0, norm_loudness=True)
        except Exception as exc:
            raise OpError("voice_failed", f"{type(exc).__name__}: {exc}", True) from exc
        self.voices[voice_id] = self.model.conds
        return {"voice_id": voice_id}

    def op_synth(self, req):
        chunk = require(req, "chunk", int)
        require(req, "total", int)
        voice_id = require(req, "voice_id", str)
        text = require(req, "text", str)
        seed = require(req, "seed", int)

        fail_chunk = os.environ.get("NARRATE_TEST_FAIL_CHUNK")
        crash_chunk = os.environ.get("NARRATE_TEST_CRASH_CHUNK")
        if crash_chunk and crash_chunk.isdigit() and int(crash_chunk) == chunk:
            os._exit(3)
        if fail_chunk and fail_chunk.isdigit() and int(fail_chunk) == chunk:
            raise OpError("test_failure", f"test failure injected for chunk {chunk}")
        if self.model is None or voice_id not in self.voices:
            raise OpError("bad_request", f"voice {voice_id!r} is not prepared")

        import torch

        self.model.conds = self.voices[voice_id]
        torch.manual_seed(seed)
        started = time.monotonic()
        try:
            # [FR-018] Requirement 6: library defaults; generate() applies the PerTh watermark.
            wav = self.model.generate(text)
        except Exception as exc:
            if is_oom(exc):
                release_cuda()
                raise OpError("out_of_memory", f"out of memory on chunk {chunk}", True) from exc
            raise OpError("synth_failed", f"{type(exc).__name__}: {exc}", True) from exc
        elapsed = time.monotonic() - started
        count, hex_text = to_hex(wav)
        if count < 1:
            raise OpError("synth_failed", "the model returned no samples")
        return {
            "chunk": chunk,
            "samples": count,
            "sample_rate": SAMPLE_RATE,
            "synth_seconds": elapsed,
            "pcm_f32le_hex": hex_text,
        }

    def op_kokoro_clip(self, req):
        model_dir = require(req, "model_dir", str)
        voice = require(req, "voice", str)
        text = require(req, "text", str)
        requested = require(req, "device", str)
        import torch

        device, _ = self.resolve_device(requested, torch)
        kmodel = pipeline = voice_tensor = None
        try:
            import numpy as np
            from kokoro import KModel, KPipeline

            kmodel = KModel(
                repo_id=KOKORO_REPO,
                config=os.path.join(model_dir, "config.json"),
                model=os.path.join(model_dir, "kokoro-v1_0.pth"),
            )
            kmodel = kmodel.to(device).eval()
            pipeline = KPipeline(lang_code="a", repo_id=KOKORO_REPO, model=kmodel)
            voice_tensor = torch.load(os.path.join(model_dir, "voices", f"{voice}.pt"), weights_only=True)
            torch.manual_seed(0)
            parts = [r.audio.detach().cpu().float().numpy().reshape(-1) for r in pipeline(text, voice=voice_tensor, speed=1) if r.audio is not None]
            if not parts:
                raise RuntimeError("Kokoro produced no audio")
            count, hex_text = to_hex(np.concatenate(parts))
        except OpError:
            raise
        except Exception as exc:
            if is_oom(exc):
                raise OpError("out_of_memory", "out of memory while rendering the Kokoro clip", True) from exc
            raise OpError("kokoro_failed", f"{type(exc).__name__}: {exc}", True) from exc
        finally:
            del kmodel, pipeline, voice_tensor
            release_cuda()
        return {"samples": count, "sample_rate": SAMPLE_RATE, "device": device, "pcm_f32le_hex": hex_text}

    def op_download(self, req):
        repo = require(req, "repo", str)
        revision = require(req, "revision", str)
        filename = require(req, "file", str)
        local_dir = require(req, "local_dir", str)
        try:
            from huggingface_hub import hf_hub_download

            path = hf_hub_download(repo_id=repo, filename=filename, revision=revision, local_dir=local_dir)
        except Exception as exc:
            raise OpError("download_failed", f"download of {repo}/{filename} failed: {type(exc).__name__}: {exc}", True) from exc
        return {"path": os.path.abspath(path)}

    def op_shutdown(self, req):
        return {}


def require(req, key, kind):
    value = req.get(key)
    ok = isinstance(value, kind) and not (kind is int and isinstance(value, bool))
    if not ok:
        raise OpError("bad_request", f"field {key!r} is missing or not a {kind.__name__}")
    return value


OPS = {
    "hello": Session.op_hello,
    "probe": Session.op_probe,
    "load": Session.op_load,
    "voice": Session.op_voice,
    "synth": Session.op_synth,
    "kokoro_clip": Session.op_kokoro_clip,
    "download": Session.op_download,
    "shutdown": Session.op_shutdown,
}


def handle(session, line):
    """Return (reply, stop). Never raises."""
    request_id = None
    try:
        req = json.loads(line)
        if not isinstance(req, dict):
            raise OpError("bad_request", "request is not a JSON object")
        rid = req.get("id")
        request_id = rid if isinstance(rid, int) and not isinstance(rid, bool) else None
        if request_id is None:
            raise OpError("bad_request", "request has no integer id")
        op = req.get("op")
        handler = OPS.get(op) if isinstance(op, str) else None
        if handler is None:
            raise OpError("bad_request", f"unknown op {op!r}")
        body = handler(session, req)
        reply = {"id": request_id, "ok": True}
        reply.update(body)
        return reply, op == "shutdown"
    except OpError as err:
        tb = traceback.format_exc() if err.with_traceback else None
        return error_reply(request_id, err.code, err.message, tb), False
    except Exception as exc:  # last resort: a bug in an op must not kill the session
        log(f"unexpected error: {exc}")
        return error_reply(request_id, "bad_request", f"internal worker error: {type(exc).__name__}: {exc}", traceback.format_exc()), False


_FILE_TYPE_PIPE = 3
_POLL_SECONDS = 0.005


def stdin_blocks():
    """Yield raw byte blocks from stdin until end-of-file.

    A thread parked in a blocking read of a Windows pipe stalls the main thread's import of
    numpy and torch (the loader waits on the same handle; observed with Python 3.11.14), so
    when stdin is a pipe it is polled with PeekNamedPipe and read only when data is ready.
    """
    import ctypes
    import msvcrt
    from ctypes import wintypes

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    handle = wintypes.HANDLE(msvcrt.get_osfhandle(0))
    kernel32.GetFileType.argtypes = [wintypes.HANDLE]
    kernel32.GetFileType.restype = wintypes.DWORD
    if kernel32.GetFileType(handle) != _FILE_TYPE_PIPE:
        while True:  # console or file stdin (manual runs): a blocking read is fine
            block = os.read(0, 1 << 20)
            if not block:
                return
            yield block
    kernel32.PeekNamedPipe.argtypes = [
        wintypes.HANDLE, ctypes.c_void_p, wintypes.DWORD, ctypes.c_void_p, ctypes.POINTER(wintypes.DWORD), ctypes.c_void_p,
    ]
    kernel32.PeekNamedPipe.restype = wintypes.BOOL
    available = wintypes.DWORD(0)
    while True:
        if not kernel32.PeekNamedPipe(handle, None, 0, None, ctypes.byref(available), None):
            return  # broken pipe: the parent closed its end
        if available.value:
            block = os.read(0, min(available.value, 1 << 20))
            if not block:
                return
            yield block
        else:
            time.sleep(_POLL_SECONDS)


def main():
    signal.signal(signal.SIGINT, signal.SIG_IGN)
    requests = queue.Queue()

    def read_stdin():
        # [FR-005] Requirement 2: parent gone means exit now, even in the middle of a request.
        pending = b""
        try:
            for block in stdin_blocks():
                pending += block
                while b"\n" in pending:
                    raw, pending = pending.split(b"\n", 1)
                    requests.put(raw.decode("utf-8", errors="replace"))
        finally:
            os._exit(0)

    threading.Thread(target=read_stdin, name="stdin-reader", daemon=True).start()
    session = Session()
    while True:
        line = requests.get().strip()
        if not line:
            continue
        reply, stop = handle(session, line)
        send(reply)
        if stop:
            os._exit(0)


if __name__ == "__main__":
    main()
