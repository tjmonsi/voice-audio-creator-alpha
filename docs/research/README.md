# Research notes for `narrate` and `narrate-onnx`

Checked 2026-10-12. Every fact carries a source URL, a pinned-artifact file and line, or a test run on this machine; anything else is marked unverified.

| File | Covers |
|---|---|
| `phase1-python-runtime.md` | chatterbox-tts 0.1.7 Turbo API, Hugging Face pins and sha256, PyTorch CUDA wheels, uv project and lock, Kokoro-82M, pitch check, Rust crates, hash enforcement, cache locations, VRAM measurement |
| `phase2-onnx-runtime.md` | `ResembleAI/chatterbox-turbo-ONNX` graphs and generation loop, tokenizer in Rust (tested), variant sizes and sha256, `ort` crate linking, official ONNX Runtime and CUDA artifacts, EP kernel coverage, PerTh port search |
| `installer.md` | Inno Setup 6 via winget, per-user directives, user PATH add and remove, uninstall cleanup, silent flags |

## Pins

| Artifact | Pin |
|---|---|
| `chatterbox-tts` | 0.1.7, wheel sha256 `83782500e3ad4e7c919132e9d7eb8755f29f57c5bde5ec48c655ca23a4eb113c` |
| `kokoro` / `misaki[en]` | 0.9.4 / 0.9.4 |
| torch / torchaudio | 2.6.0+cu124 from `https://download.pytorch.org/whl/cu124` |
| Python | CPython 3.11.14 via `uv python install` |
| `ResembleAI/chatterbox-turbo` | `749d1c1a46eb10492095d68fbcf55691ccf137cd` |
| `hexgrad/Kokoro-82M` | `f3ff3571791e39611d31c381e3a41a3af07b4987` |
| `ResembleAI/chatterbox-turbo-ONNX` | `d21799bd0354adb85e348b8a0442a8405110a2cf` |
| `ort` | `=2.0.0-rc.13` (no stable 2.x exists) with `load-dynamic` |
| ONNX Runtime | Microsoft 1.28.3 Windows GPU zip (CUDA 13 or CUDA 12 build) |
| `tokenizers` (Rust) | 0.23.2, `default-features = false, features = ["fancy-regex"]` |
| Inno Setup | `JRSoftware.InnoSetup` 6.7.3 |

## Issues the spec must handle

1. resemble-perth needs `pkg_resources`; pin `setuptools==81.0.0` or Turbo fails to load in a uv env.
2. misaki downloads `en_core_web_sm` at run time; lock it as a URL dependency.
3. Bind the PyTorch index to torch/torchaudio only (`explicit = true`).
4. `from_pretrained` cannot pin a revision; download pinned files, verify sha256, use `from_local`.
5. Each Turbo call stops at 40 s of speech; chunk the script.
6. The Python libraries print to stdout and piped stdio defaults to cp1252; protect the JSON-lines channel and run with `-I -X utf8`.
7. Cache one voice conditioning per voice; `prepare_conditionals` replaces the built-in voice.
8. For ONNX on CUDA, use the fp16 or q4f16 language model; the CUDA GQA kernel has no float32 version.
9. Load `onnxruntime.dll` by absolute path; Windows ships an older 1.17 copy in System32.
10. ORT's CUDA EP needs CUDA 13 (or 12.8+) and cuDNN 9 DLLs that the official zip does not include.
11. No official PerTh ONNX or Rust port exists, so `narrate-onnx` output is not watermarked.
12. Other apps held 6.9 of 8 GB VRAM when checked; GPU tests need a VRAM baseline.
