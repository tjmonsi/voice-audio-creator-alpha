# Phase 1 research: Python runtime for `narrate`

Checked 2026-10-12. Labels: **tested** = run on this machine; **code-read** = read from the pinned artifact (wheel or HF file at a pinned commit); **doc** = vendor documentation; **unverified** = not confirmed. Scratch evidence was produced in a throwaway directory; nothing was installed into the system or the repo.

## Findings that change the design

| # | Finding | Evidence | Action for the spec |
|---|---|---|---|
| F1 | `perth.PerthImplicitWatermarker` silently becomes `None` when `pkg_resources` is missing, and `ChatterboxTurboTTS.__init__` then fails with `TypeError: 'NoneType' object is not callable`. uv venvs do not include setuptools; setuptools 82.0.0 removed `pkg_resources`. | code-read: `perth/__init__.py` lines 12-15 (try/except ImportError -> None); `perth/perth_net/__init__.py` line 1 `from pkg_resources import resource_filename`. tested: `pkg_resources/__init__.py` present in setuptools 80.9.0 and 81.0.0 wheels, absent in 82.0.0. Reported upstream: https://github.com/resemble-ai/Perth/issues/7 , https://github.com/resemble-ai/chatterbox/issues/198 | Pin `setuptools==81.0.0` as a direct dependency of the worker env. |
| F2 | misaki (Kokoro G2P) runs `spacy.cli.download("en_core_web_sm")` at runtime if the model package is missing. That pip-installs from GitHub at runtime and needs pip, which uv venvs lack. | code-read: misaki 0.9.4 `en.py` lines 499-503. kokoro 0.9.4 `__main__.py` docstring: "pip not installed: `uv pip install pip`". | Add `en-core-web-sm` 3.8.0 as a locked URL dependency from the official explosion/spacy-models release (hash below). |
| F3 | PyPI's Windows `torch==2.6.0` wheel is CPU-only (204 MB vs 2.53 GB for `+cu124`). With `--index <pytorch> --index-strategy unsafe-best-match`, uv also took numpy, pillow, jinja2, sympy, networkx, mpmath, colorama, typing-extensions from the PyTorch index. With an `explicit = true` index bound to torch/torchaudio only, just those two came from it. | tested: `uv pip compile --emit-index-annotation` (10 vs 2 packages from download.pytorch.org). Sizes from PyPI JSON and HEAD requests. | Use the pyproject below (explicit index), not `--extra-index-url`/`unsafe-best-match`. |
| F4 | `ChatterboxTurboTTS.from_pretrained(device)` has no `revision` argument and downloads `*.safetensors, *.json, *.txt, *.pt, *.model` from `main`, including the unused 1.06 GB `s3gen.safetensors` (4.04 GB total). | code-read: `tts_turbo.py` lines 186-202. HF API sizes. | Download yourself with `snapshot_download(revision=<sha>, allow_patterns=[...])` into a local dir, verify sha256, then call `ChatterboxTurboTTS.from_local(dir, device)` (2.99 GB). |
| F5 | One `generate()` call is capped at 1000 speech tokens = 40 s of audio, and text is silently truncated at 1024 tokens. No internal chunking. | code-read: `t3.py` line 416 `max_gen_len=1000`; `s3tokenizer.py` line 18 `S3_TOKEN_RATE = 25`; `tts_turbo.py` line 271 `truncation=True`; HF `tokenizer_config.json` `model_max_length: 1024`. | `narrate` must chunk text so each chunk speaks well under 40 s (see section 1.6). |
| F6 | The libraries print to stdout (`print(...)` in `tts_turbo.py` lines 176, 190, 192, 213; `t3.py` line 473; `s3gen.py` line 136; kokoro `pipeline.py` line 84 and `model.py` line 41 when `repo_id` is None). tqdm bars go to stderr. Piped Python stdio on this machine is cp1252. | code-read; tested: `python -I -c "import sys;print(sys.stdout.encoding)" \| cat` printed `cp1252`; with `-I -X utf8` it printed `utf-8`; `PYTHONUTF8=1` is ignored under `-I`. | Worker: start with `python -I -X utf8 -u`; keep a private handle to the real stdout for JSON lines and point `sys.stdout` at stderr; set `TQDM_DISABLE=1` if bars are unwanted. |
| F7 | `prepare_conditionals()` overwrites `model.conds`, so after one cloned-voice call, a later call without `audio_prompt_path` no longer uses the built-in `conds.pt` voice. | code-read: `tts_turbo.py` lines 178-181, 246, 261-264. | Cache one `Conditionals` per voice (built-in loaded via `Conditionals.load(conds.pt)`) and assign `model.conds` before each chunk. |
| F8 | At check time other applications held 6927 of 8192 MiB VRAM on the RTX 3050 (WDDM). | tested: `nvidia-smi --query-gpu=...` | QA must free VRAM before GPU runs; report device-wide baseline with every measurement. |

## 1. chatterbox-tts 0.1.7 and `ChatterboxTurboTTS`

### 1.1 Package identity

| Item | Value | Source |
|---|---|---|
| Version / Python | 0.1.7, `requires_python >=3.10`; README: "developed and tested ... on Python 3.11" | https://pypi.org/pypi/chatterbox-tts/0.1.7/json ; README at commit below |
| Wheel | `chatterbox_tts-0.1.7-py3-none-any.whl`, 108543 B, sha256 `83782500e3ad4e7c919132e9d7eb8755f29f57c5bde5ec48c655ca23a4eb113c`, uploaded 2026-03-26 | PyPI JSON |
| sdist | sha256 `ed8afae83819b40a25927c2ef3bcc67f928bdfcf434c1376c972e6039252a187` | PyPI JSON |
| Git source | commit `59bc590b3cad826e5d5987745bf6844627a21ad5` (2026-03-26, "Broaden dependency version ranges for Python 3.13+ compatibility (#495)"); its `src/chatterbox/tts_turbo.py` is byte-identical to the wheel (tested with diff). Default branch is `master`; master has later unreleased changes (Nano model) still labelled 0.1.7. | https://github.com/resemble-ai/chatterbox/commit/59bc590b3cad826e5d5987745bf6844627a21ad5 |
| License | MIT | PyPI metadata; GitHub API |

Wheel `requires_dist` (code-read): `numpy>=1.24.0,<2.0.0` (py<3.13), `librosa==0.11.0`, `s3tokenizer` (unpinned), `torch==2.6.0` / `torchaudio==2.6.0` (py<3.14), `transformers==5.2.0`, `diffusers==0.29.0`, `resemble-perth>=1.0.0`, `conformer==0.3.2`, `safetensors==0.5.3`, `spacy-pkuseg`, `pykakasi==2.3.0`, `gradio==6.8.0`, `pyloudnorm`, `omegaconf`. The git commit's pyproject instead says `resemble-perth @ git+https://github.com/resemble-ai/Perth.git@master`; the PyPI wheel is what gets installed.

Line numbers below refer to `chatterbox/tts_turbo.py` in the 0.1.7 wheel unless stated.

### 1.2 Loading

| API | Behavior |
|---|---|
| `REPO_ID = "ResembleAI/chatterbox-turbo"` | line 26 |
| `ChatterboxTurboTTS.from_pretrained(cls, device)` | lines 186-202. If `device == "mps"` and MPS is unavailable, prints a message and uses `"cpu"`. Calls `snapshot_download(repo_id=REPO_ID, token=os.getenv("HF_TOKEN") or None, allow_patterns=["*.safetensors", "*.json", "*.txt", "*.pt", "*.model"])` with no revision, then `from_local`. No CUDA fallback. |
| `ChatterboxTurboTTS.from_local(cls, ckpt_dir, device)` | lines 133-183. Loads `ve.safetensors`, `t3_turbo_v1.safetensors` (GPT2_medium config, `speech_tokens_dict_size=6563`, `speech_cond_prompt_len=375`), `s3gen_meanflow.safetensors` (`S3Gen(meanflow=True)`), tokenizer via `AutoTokenizer.from_pretrained(ckpt_dir)`, and `conds.pt` if present (`Conditionals.load`, `torch.load(..., weights_only=True)`). `map_location` is CPU for `cpu`/`mps`, else `None`. `t3_turbo_v1.yaml` and `s3gen.safetensors` are not read. All weights stay fp32; no dtype option. |
| Files `from_local` needs | `ve.safetensors`, `t3_turbo_v1.safetensors`, `s3gen_meanflow.safetensors`, `conds.pt`, `vocab.json`, `merges.txt`, `tokenizer_config.json`, `special_tokens_map.json`, `added_tokens.json` |
| Device strings | `"cuda"`, `"cpu"`, `"mps"`; passed straight to `.to(device)`. The PerTh watermarker always runs on CPU (`PerthImplicitWatermarker(device="cpu")` default, perth `perth_watermarker.py` line 17-18). |
| Import cost | No `gradio` import anywhere in the package (tested: grep found 0 references); `gradio==6.8.0` is only a declared dependency (24 MB wheel plus fastapi, uvicorn, pandas, etc.). `import chatterbox.tts_turbo` runs `chatterbox/__init__.py`, which imports `tts`, `vc`, `mtl_tts`, so torch, librosa, transformers, diffusers, perth, s3tokenizer (which imports `onnx`) load at import time. `pykakasi` and `spacy_pkuseg` are imported lazily (tokenizer.py lines 81, 193). Import time: unverified. |

### 1.3 `generate()` (lines 248-296)

```python
def generate(self, text, repetition_penalty=1.2, min_p=0.00, top_p=0.95,
             audio_prompt_path=None, exaggeration=0.0, cfg_weight=0.0,
             temperature=0.8, top_k=1000, norm_loudness=True)
```

- With `audio_prompt_path`: calls `prepare_conditionals(path, exaggeration, norm_loudness)` (line 262). Without it: `assert self.conds is not None` (line 264), so the built-in `conds.pt` voice is used if no clip was prepared earlier (see F7).
- `cfg_weight`, `exaggeration`, `min_p` > 0 only log a warning: "CFG, min_p and exaggeration are not supported by Turbo version and will be ignored." (lines 266-267).
- Text goes through `punc_norm` (lines 29-65): empty text becomes "You need to add some text for me to talk."; first letter capitalised; whitespace collapsed; `…`->`, `, `:`->`,`, em/en dash->`-`, curly quotes->straight; appends `.` unless text ends with `. ! ? - ,`.
- Tokenize with `truncation=True` (line 271), then `t3.inference_turbo(...)` (lines 274-281). Sampling order in `t3.py` lines 418-426: Temperature, TopK, TopP, RepetitionPenalty (HF logits processors), then `torch.multinomial`. Start token 6561, stop 6562 (`t3_config.py` lines 11-12); `max_gen_len=1000`.
- Tokens >= 6561 removed, three silence tokens `S3GEN_SIL = 4299` appended (lines 284-287), `s3gen.inference(..., n_cfm_timesteps=2)` (lines 289-293).
- Watermark: `self.watermarker.apply_watermark(wav, sample_rate=self.sr)` on every call (line 295).
- Returns a CPU `torch.FloatTensor` of shape `(1, n_samples)`. Output is not loudness-normalised (`norm_loudness` applies to the reference clip only).
- Randomness: `torch.multinomial` in T3 and `torch.randn_like` / `torch.randn` in S3Gen (`flow_matching.py` line 63, `s3gen.py` line 316). Seed `torch.manual_seed` per chunk for repeatable output.

### 1.4 Sample rate

`model.sr = S3GEN_SR = 24000` (line 123; `models/s3gen/const.py` line 1). Output WAV should be 24 kHz mono.

### 1.5 Reference clip rules (`prepare_conditionals`, lines 217-246)

| Rule | Value | Line |
|---|---|---|
| Load | `librosa.load(path, sr=24000)` (mono, resampled) | 219 |
| Minimum length | `assert len/sr > 5.0` ("Audio prompt must be longer than 5 seconds!"); 5.0 s exactly fails | 221 |
| Loudness | normalised to -27 LUFS with pyloudnorm when `norm_loudness=True` | 204-215, 223-224 |
| Decoder conditioning | first 10 s at 24 kHz (`DEC_COND_LEN = 10 * S3GEN_SR`) | 112, 228 |
| T3 prompt tokens | first 15 s at 16 kHz (`ENC_COND_LEN = 15 * S3_SR`), max 375 tokens | 111, 232-235 |
| Speaker embedding | voice encoder over the whole 16 kHz clip | 238-239 |

A 10-15 s clean clip uses every conditioning path fully. Plan the Kokoro clips at about 12 s.

### 1.6 Per-call limits and chunking

- Speech: 1000 tokens x 1/25 s = 40 s maximum per call; generation stops early at token 6562.
- Text: 1024 tokens, silently truncated.
- Recommendation (estimate, not measured): keep each chunk to 1-3 sentences and about 300 characters (about 20 s at typical narration pace) so the 40 s cap is never reached. Measure actual seconds per character in QA.

### 1.7 Paralinguistic tags

The tokenizer adds 19 tokens (`added_tokens.json` at the pinned commit): `[angry]` 50257, `[fear]` 50258, `[surprised]` 50259, `[whispering]` 50260, `[advertisement]` 50261, `[dramatic]` 50262, `[narration]` 50263, `[crying]` 50264, `[happy]` 50265, `[sarcastic]` 50266, `[clear throat]` 50267, `[sigh]` 50268, `[shush]` 50269, `[cough]` 50270, `[groan]` 50271, `[sniff]` 50272, `[gasp]` 50273, `[chuckle]` 50274, `[laugh]` 50275. Tags must match exactly (lowercase; one space in `[clear throat]`). The model card names `[cough]`, `[laugh]`, `[chuckle]` "and more"; how well the others render is undocumented. `punc_norm` leaves square brackets intact.

## 2. Hugging Face repos: pinned revisions, files, offline use

Commit SHAs from `https://huggingface.co/api/models/<repo>?blobs=true` on 2026-10-12.

| Repo | Commit SHA (pin this) | lastModified | License / gated |
|---|---|---|---|
| `ResembleAI/chatterbox-turbo` | `749d1c1a46eb10492095d68fbcf55691ccf137cd` | 2025-12-15 | MIT / no |
| `hexgrad/Kokoro-82M` | `f3ff3571791e39611d31c381e3a41a3af07b4987` | 2025-04-10 | Apache-2.0 / no |
| `ResembleAI/chatterbox-turbo-ONNX` | `d21799bd0354adb85e348b8a0442a8405110a2cf` | 2025-12-15 | MIT / no |

### 2.1 Files to fetch for phase 1 (sha256)

LFS hashes are the API `lfs.sha256`; small-file hashes were computed from files downloaded at the pinned commit (tested).

| Repo file | Bytes | sha256 |
|---|---:|---|
| turbo `t3_turbo_v1.safetensors` | 1915480052 | `fcf1f8c1d651bb7e3acd69ee5be269b4ac10c02980b7708213d598bc9f7cdf87` |
| turbo `s3gen_meanflow.safetensors` | 1064875036 | `d65cb687a2ed581ee6cc297e919ffefa63386944f42364ae13b78a594945514f` |
| turbo `ve.safetensors` | 5695784 | `f0921cab452fa278bc25cd23ffd59d36f816d7dc5181dd1bef9751a7fb61f63c` |
| turbo `conds.pt` | 169454 | `b1852099306fd6a7814eb9d0bd10186caba7249596cc23868f78a0eefbfa5033` |
| turbo `vocab.json` | 999186 | `f6bd25a65e4e63ca31360e9fb11c7e4f9a391a78385d640acd814092dd6eee4f` |
| turbo `merges.txt` | 456318 | `1ce1664773c50f3e0cc8842619a93edc4624525b728b188a9e0be33b7726adc5` |
| turbo `tokenizer_config.json` | 3878 | `bca16a2ac1ddbd78b8d6228f0031884cc74b6ea54b967d6f6d2ebae9ccde23e6` |
| turbo `special_tokens_map.json` | 470 | `92ba8063bf40aa163eadebbfe0de07c2aebe44cf0d4a9e8726580b0781fd2640` |
| turbo `added_tokens.json` | 418 | `72e4ab6acb0d9309ac3df4b526ae5fd80a2da5bc5ab7bb02d85096a374f69193` |
| kokoro `kokoro-v1_0.pth` | 327212226 | `496dba118d1a58f5f3db2efc88dbdc216e0483fc89fe6e47ee1f2c53f18ad1e4` |
| kokoro `config.json` | 2351 | `5abb01e2403b072bf03d04fde160443e209d7a0dad49a423be15196b9b43c17f` |
| kokoro `voices/af_heart.pt` | 523425 | `0ab5709b8ffab19bfd849cd11d98f75b60af7733253ad0d67b12382a102cb4ff` |
| kokoro `voices/am_michael.pt` | 523435 | `9a443b79a4b22489a5b0ab7c651a0bcd1a30bef675c28333f06971abbd47bd37` |

Totals: turbo files above 2,987,680,596 B (about 2.99 GB); the stock `from_pretrained` patterns would add `s3gen.safetensors` (1,056,484,620 B) for 4,044,165,216 B. Kokoro files above 328,261,437 B. The `VOICES.md` SHA256 prefixes (`0ab5709b`, `9a443b79`) match.

### 2.2 Pinned download and offline loading

`huggingface_hub` 1.33.0 (the version the resolver picked) signatures (code-read `_snapshot_download.py` line 47, `file_download.py` line 784): `snapshot_download(repo_id, *, revision=None, cache_dir=None, local_dir=None, allow_patterns=None, ignore_patterns=None, local_files_only=False, token=None, ...)` and `hf_hub_download(repo_id, filename, *, subfolder=None, revision=None, cache_dir=None, local_dir=None, local_files_only=False, ...)`.

Recommended setup flow:
1. `snapshot_download("ResembleAI/chatterbox-turbo", revision="749d1c1a...", local_dir=<data>/models/chatterbox-turbo, allow_patterns=[the 9 files above])`. `local_dir` gives a plain folder with no symlinks: files are downloaded to a temp name and moved into place (or copied from an existing cache entry), with metadata under `<local_dir>/.cache/huggingface/download/` (code-read `file_download.py` lines 1313-1515, `_local_folder.py` lines 242-259). Without `local_dir`, Windows without Developer Mode cannot create cache symlinks (doc below).
2. Verify each file's sha256 against section 2.1; abort on mismatch. `huggingface_hub` itself only checks the size ("Consistency check failed: file should be of size ...", `file_download.py` line 443), not the content hash.
3. At run time set `HF_HUB_OFFLINE=1` and call `ChatterboxTurboTTS.from_local(<dir>, device)`; nothing contacts the Hub (`from_local` and `AutoTokenizer.from_pretrained(<local dir>)` read only local files).

HTTP facts (tested with HEAD on `https://huggingface.co/<repo>/resolve/<sha>/<file>`): response header `X-Repo-Commit` echoes the pinned SHA; `X-Linked-ETag` is the sha256 for LFS files (conds.pt: `b1852099...`) and the git blob SHA-1 for small files (tokenizer_config.json: `58bc759e...`, equal to `git hash-object`). The API `?blobs=true` also returns `blobId` (git SHA-1) for every file.

Environment variables (doc: https://huggingface.co/docs/huggingface_hub/package_reference/environment_variables; "All environment variables are read at import time"):

| Variable | Effect |
|---|---|
| `HF_HUB_OFFLINE=1` | "no HTTP calls will be made to the Hugging Face Hub"; only cached files are used; error if missing. `TRANSFORMERS_OFFLINE` is honoured as an alias (code-read `constants.py` line 194). |
| `HF_HOME` | root for token and caches; default `~/.cache/huggingface` |
| `HF_HUB_CACHE` | repo cache; default `$HF_HOME/hub` |
| `HF_XET_CACHE`, `HF_ASSETS_CACHE` | default `$HF_HOME/xet`, `$HF_HOME/assets` |
| `HF_HUB_DISABLE_TELEMETRY=1` (or `DO_NOT_TRACK=1`) | disables HF library telemetry |
| `HF_HUB_DISABLE_SYMLINKS_WARNING=1` | silences the Windows no-symlink warning |
| `HF_HUB_DISABLE_PROGRESS_BARS=1` | no tqdm bars from downloads |

## 3. PyTorch CUDA wheels and the uv environment

### 3.1 Wheels (tested against https://download.pytorch.org/whl/<variant>/torch/)

| Variant | `torch-2.6.0+<v>-cp311-cp311-win_amd64.whl` | Bytes | torchaudio cp311 win |
|---|---|---:|---|
| cu118 | yes, sha256 `6ab0417ce9b78ab0a34721a99734b5fd4cc3d7b62ff1c068a7d636fd829772db` | 2728868903 | yes |
| cu121 | no 2.6.0 build | - | - |
| cu124 | yes, sha256 `6a1fb2714e9323f11edb6e8abf7aad5f79e45ad25c081cde87681a18d99c29eb` | 2532350702 | yes, `a25e146ce66ea9a6aed39008cc2001891bdf75253af479a4c32096678b2073b3`, 4163304 B |
| cu126 | yes, sha256 `5ddca43b81c64df8ce0c59260566e648ee46b2622ab6a718e38dea3c0ca059a1` | 2496131084 | yes |
| cpu | yes | 206540444 | yes |

- cp310, cp312, cp313 Windows wheels also exist for cu124 and cu126.
- Official install commands for 2.6.0 list cu118, cu124, cu126 and cpu index URLs (pytorch.github.io source `_get_started/previous-versions.md`, "v2.6.0").
- The cu124 Windows wheel bundles CUDA 12.4 runtime and cuDNN 9 DLLs in `torch/lib` (tested by listing the wheel's zip directory: `cudart64_12.dll`, `cublas64_12.dll`, `cublasLt64_12.dll`, `cufft64_11.dll`, `curand64_10.dll`, `cudnn64_9.dll` plus `cudnn_*64_9.dll`, `nvrtc64_120_0.dll`). No CUDA toolkit install is needed for phase 1.
- Driver 610.88 (R610 branch) is newer than any CUDA 12.x requirement, so cu124 and cu126 both run (NVIDIA driver branch table, https://docs.nvidia.com/cuda/cuda-toolkit-release-notes/index.html ; RTX 3050 is Ampere).
- Recommendation: cu124 (conservative; cu126 is equally valid and 36 MB smaller).

### 3.2 Recommended project files (resolution tested)

`python/pyproject.toml` (shipped with `narrate`, plus a committed `uv.lock`):

```toml
[project]
name = "narrate-worker"
version = "0.1.0"
requires-python = "==3.11.*"
dependencies = [
  "chatterbox-tts==0.1.7",
  "kokoro==0.9.4",
  "misaki[en]==0.9.4",
  "torch==2.6.0",
  "torchaudio==2.6.0",
  "setuptools==81.0.0",   # F1: pkg_resources for resemble-perth
  "en-core-web-sm",       # F2: avoid runtime spacy download
]

[tool.uv]
environments = ["sys_platform == 'win32' and platform_machine == 'AMD64'"]
package = false

[tool.uv.sources]
torch = { index = "pytorch-cu124" }
torchaudio = { index = "pytorch-cu124" }
en-core-web-sm = { url = "https://github.com/explosion/spacy-models/releases/download/en_core_web_sm-3.8.0/en_core_web_sm-3.8.0-py3-none-any.whl" }

[[tool.uv.index]]
name = "pytorch-cu124"
url = "https://download.pytorch.org/whl/cu124"
explicit = true
```

What was tested: `uv pip compile pyproject.toml --python-version 3.11 --python-platform x86_64-pc-windows-msvc --no-build --generate-hashes` (uv 0.10.2) resolved 153 packages without building any sdist; every pinned package has a cp311 `win_amd64`, abi3, or pure-Python wheel (checked against PyPI JSON). `en_core_web_sm-3.8.0` wheel sha256 `1932429db727d4bff3deed6b34cfc05df17794f4a52eeb26cf8928f7c1a0fb85`. Not tested: `uv lock` itself, because it needs a Python 3.11 interpreter (`error: No interpreter found for Python ==3.11.*`), which was not installed. The developer runs `uv python install 3.11.14` then `uv lock`.

Key pins from that resolution (expect the same in `uv.lock` if locked soon): torch 2.6.0+cu124, torchaudio 2.6.0+cu124, transformers 5.2.0, tokenizers 0.22.2, huggingface-hub 1.33.0, diffusers 0.29.0, numpy 1.26.4, librosa 0.11.0, safetensors 0.5.3, s3tokenizer 0.3.0, resemble-perth 1.0.1, spacy 3.8.16, spacy-pkuseg 1.0.1, espeakng-loader 0.2.4, phonemizer-fork 3.3.2, soundfile 0.14.0, gradio 6.8.0, setuptools 81.0.0, onnx 1.23.2, scipy 1.17.1, numba 0.68.0.

- `s3tokenizer` is unpinned upstream; 0.3.0 (2025-12-22) is its first release with a wheel (all earlier releases are sdist-only), and it contains `s3tokenizer.model_v2.S3TokenizerV2`, `ModelConfig`, and `utils.padding` that chatterbox imports (code-read). Locking pins it.
- Static check (not runtime-tested): every `huggingface_hub` name diffusers 0.29.0 imports exists in huggingface_hub 1.33.0.

### 3.3 Setup commands (uv 0.10.2 flags confirmed with `--help`)

```text
set UV_CACHE_DIR=%LOCALAPPDATA%\narrate\uv-cache
set UV_PYTHON_INSTALL_DIR=%LOCALAPPDATA%\narrate\python
set UV_PROJECT_ENVIRONMENT=%LOCALAPPDATA%\narrate\venv
set UV_NO_CONFIG=1
uv python install 3.11.14 --no-bin --no-registry
uv sync --project <install dir>\python --frozen --managed-python --python 3.11.14 --no-progress
```

- `--frozen`: "Sync without updating the `uv.lock` file"; `--locked`: "Assert that the `uv.lock` will remain unchanged" (errors if pyproject and lock disagree). Use `uv lock --check` ("Check if the lockfile is up-to-date") in development and `--frozen` at user setup.
- `--no-registry`: "Do not register the Python installation in the Windows registry"; `--no-bin`: "Do not install a Python executable into the `bin` directory". Without them uv writes outside the data dir.
- uv 0.10.2 lists `cpython-3.11.14-windows-x86_64-none` as the 3.11 download (tested `uv python list 3.11 --only-downloads`).
- `UV_PROJECT_ENVIRONMENT` puts the venv outside the project folder (tested in a scratch project).
- Latest uv is 0.13.0 (2026-10-09); this machine has 0.10.2. Official Windows zip for 0.10.2: `https://github.com/astral-sh/uv/releases/download/0.10.2/uv-x86_64-pc-windows-msvc.zip`, sha256 `493ebbe0e06128d6ee4905e1ed5e2a433fb0f7cfc08b0eaca9fab4ca76778ae1` (GitHub release asset digest), if the installer bundles uv.

### 3.4 Windows install problems checked

| Concern | Status |
|---|---|
| spacy-pkuseg needs a compiler | No: `spacy_pkuseg-1.0.1-cp311-cp311-win_amd64.whl` exists (tested). |
| s3tokenizer sdist-only | Only before 0.3.0; 0.3.0 has a wheel. |
| Any sdist-only dependency for cp311 win | None in the 153-package resolution (`--no-build` succeeded; wheel check passed). |
| soundfile needs libsndfile | `soundfile-0.14.0-py2.py3-none-win_amd64.whl` (1.0 MB) bundles it. |
| perth `None` under uv | F1. |
| CPU torch from PyPI | F3. |

Install size (estimate from wheel sizes): torch cu124 2.53 GB + other wheels about 0.31 GB + Python 3.11 runtime; models 2.99 GB + 0.33 GB. About 6.2 GB downloaded in total.

## 4. Kokoro-82M (reference clip generator)

| Item | Value | Source |
|---|---|---|
| PyPI `kokoro` | 0.9.4 (2025-04-05), `requires_python <3.13,>=3.10`, Apache License; deps `huggingface-hub, loguru, misaki[en]>=0.9.4, numpy, torch, transformers` (all unpinned) | https://pypi.org/pypi/kokoro/json |
| `misaki[en]` 0.9.4 | adds `espeakng-loader`, `num2words`, `phonemizer-fork`, `spacy`, `spacy-curated-transformers` | https://pypi.org/pypi/misaki/json |
| espeak-ng on Windows | Not needed separately: `espeakng_loader-0.2.4-py3-none-win_amd64.whl` contains `espeakng_loader/espeak-ng.dll` and `espeak-ng-data/` (tested listing). misaki `espeak.py` lines 8-10 point phonemizer at those paths. The kokoro GitHub README still tells Windows users to install the espeak-ng MSI; by code reading that is unnecessary with misaki 0.9.4. espeak is only the fallback for out-of-dictionary English words (`pipeline.py` lines 106-111 log a warning and continue if it fails). |
| phonemizer | `phonemizer-fork` 3.3.2 has `EspeakWrapper.set_data_path` (code-read `wrapper.py` line 119); installing the original `phonemizer` instead breaks misaki (https://github.com/hexgrad/kokoro/issues/206). |
| Licenses | kokoro, misaki "Apache License" (PyPI metadata); phonemizer-fork GPL-3.0 (PyPI metadata); espeak-ng GPL-3.0 (GitHub API for `espeak-ng/espeak-ng`), whose DLL `espeakng-loader` bundles. These are fetched at setup, not shipped in the installer. |
| Same env as chatterbox | Resolves together (section 3.2): kokoro accepts torch 2.6.0, transformers 5.2.0, numpy 1.26.4. Runtime compatibility of kokoro 0.9.4 with transformers 5.2.0 is unverified (kokoro subclasses `transformers.AlbertModel`, `modules.py` line 180). Fallback: a second uv project for Kokoro only (uv's cache hardlinks shared wheels). |
| Voices | `af_heart`: Overall Grade **A** (traits female, heart). `am_michael`: Target Quality B, Training Duration "H hours" (1-10 h), Overall Grade C+. American English uses `lang_code='a'`. | `VOICES.md` at Kokoro commit `f3ff3571...` |
| Output | 24 kHz float32 mono (model card example writes `sf.write(..., audio, 24000)`; `pipeline.py` comment "sample_rate 24000") |
| Quality range | "goldilocks range" 100-200 tokens; weak under 10-20 tokens; rushing over 400 tokens (`VOICES.md`) |

Offline, pinned usage (code-read `model.py` lines 31-67, `pipeline.py` lines 62-160):

```python
from kokoro import KModel, KPipeline
import torch, numpy as np, soundfile as sf
kmodel = KModel(repo_id="hexgrad/Kokoro-82M", config=r"<dir>\config.json",
                model=r"<dir>\kokoro-v1_0.pth").to(device).eval()
pipe = KPipeline(lang_code="a", repo_id="hexgrad/Kokoro-82M", model=kmodel)
voice = torch.load(r"<dir>\voices\af_heart.pt", weights_only=True)  # FloatTensor
audio = np.concatenate([r.audio.numpy() for r in pipe(text, voice=voice, speed=1)])
sf.write(out_wav, audio, 24000)
```

- Passing `repo_id` explicitly avoids the stdout "WARNING: Defaulting repo_id" print. Passing `config`/`model` paths and a voice tensor (or a path ending `.pt`) avoids every `hf_hub_download` call. `load_voice` splits string voices on `,`, so prefer the tensor.
- `KPipeline(lang_code='a')` builds `en.G2P(trf=False)` which needs `en_core_web_sm` (F2).
- Each result is a `KPipeline.Result` with `.graphemes`, `.phonemes`, `.audio` (iterable as a 3-tuple for backward compatibility).

## 5. Pitch (F0) check

`librosa.pyin` (librosa 0.11.0 is already in the env; doc https://librosa.org/doc/0.11.0/generated/librosa.pyin.html):
`librosa.pyin(y, *, fmin, fmax, sr=22050, frame_length=2048, hop_length=None, n_thresholds=100, beta_parameters=(2, 18), boltzmann_parameter=2, resolution=0.1, max_transition_rate=35.92, switch_prob=0.01, no_trough_prob=0.01, fill_na=nan, center=True, pad_mode='constant')`. Returns `f0, voiced_flag, voiced_prob`; unvoiced frames are NaN. Docs recommend `fmin=note_to_hz('C2')` (~65 Hz).

Recommended speech settings (not a vendor recommendation):

```python
y, sr = librosa.load(path, sr=16000, mono=True)
f0, voiced, _ = librosa.pyin(y, fmin=65.0, fmax=500.0, sr=sr, frame_length=1024)
median_f0 = float(np.nanmedian(f0[voiced]))
```

Reference ranges: "The voiced speech of a typical adult male will have a fundamental frequency from 90 to 155 Hz, and that of a typical adult female from 165 to 255 Hz." (Wikipedia "Voice frequency", revision 1327833454, citing Baken, R. J. (2000) *Clinical Measurement of Speech and Voice*, 2nd ed., p. 177, which cites Fitch and Holbrook (1970), *Archives of Otolaryngology* 92, 379-382.) The plan's thresholds (female > 165 Hz, male < 155 Hz) match these bounds; the builtin voice's median F0 is measured and reported only, with no threshold.

## 6. Rust crates for phase 1 (crates.io API, 2026-10-12)

| Need | Crate and version | Notes |
|---|---|---|
| Markdown | `pulldown-cmark` 0.13.4 (MIT, MSRV 1.71.1) | default features `getopts`, `html`; use `default-features = false` for parser-only |
| WAV | `hound` 3.5.1 (Apache-2.0, 2023-09-25) | pure Rust; `WavSpec { channels: 1, sample_rate: 24000, bits_per_sample: 16 or 32 float }` |
| CLI | `clap` 4.6.7 (MSRV 1.85), feature `derive` | |
| JSON lines | `serde` 1.0.229, `serde_json` 1.0.154 | `std::process::Command` with `Stdio::piped()` for stdin/stdout, read with `BufRead::lines()`; inherit or capture stderr |
| Data dir | `dirs` 7.0.0: `data_local_dir()` = `{FOLDERID_LocalAppData}` on Windows; join `narrate` | `directories` 6.0.0 `ProjectDirs` adds `<org>\<app>\data` levels, so it does not give `%LOCALAPPDATA%\narrate` |
| Errors | `anyhow` 1.0.104, `thiserror` 2.0.21 | |
| Hashing | `sha2` 0.11.0 (MSRV 1.85) | for verifying downloads |
| Ctrl+C | `ctrlc` 3.5.2 | |
| Tests | `assert_cmd` 2.2.2, `predicates` 3.1.4, `tempfile` 3.27.0 | |

All MSRVs are at or below Rust 1.99.

## 7. Hash verification on the Python side

- `uv.lock` stores `hash = "sha256:..."` plus `size` and `upload-time` for every sdist and wheel (tested: scratch project lock for `six==1.17.0`).
- Enforcement tested with uv 0.10.2: after changing the wheel hash in `uv.lock`, `uv sync --frozen` exited 1 with "Hash mismatch for `six==1.17.0`" and printed expected vs computed; `--locked` failed the same way; restoring the lock installed normally. uv's docs pages fetched (projects/sync, settings) do not state this explicitly, so rely on the test.
- Limit: the hash proves the file equals what was locked, not that the upstream upload was benign.
- Models: verify sha256 per section 2.1 after `snapshot_download` (the library checks size only, section 2.2); `hf_hub_download`/`snapshot_download` with a 40-hex `revision` fetch that commit (`X-Repo-Commit` header confirms).

## 8. Keeping every cache inside `%LOCALAPPDATA%\narrate`

| Tool | Variable or flag | Notes |
|---|---|---|
| uv | `UV_CACHE_DIR`, `UV_PYTHON_INSTALL_DIR`, `UV_PROJECT_ENVIRONMENT`, `UV_PYTHON_CACHE_DIR` (Python archive cache, added 0.7.0), `--no-bin` / `UV_PYTHON_INSTALL_BIN`, `--no-registry` / `UV_PYTHON_INSTALL_REGISTRY` (both added 0.8.0), `UV_NO_CONFIG=1` | `UV_TOOL_DIR` only matters if `uv tool` is used (not needed). Descriptions: https://docs.astral.sh/uv/reference/environment/ |
| Hugging Face | `HF_HOME` (also covers transformers), `HF_HUB_CACHE`, `HF_XET_CACHE`, `HF_HUB_OFFLINE=1` at run time | `local_dir` downloads avoid the cache layout |
| torch | `TORCH_HOME` | No `torch.hub` use in chatterbox, kokoro, misaki, or perth (grep); set anyway for safety |
| spaCy | none needed | `en_core_web_sm` is a locked package; misaki only downloads when `spacy.util.is_package` is false |
| misaki data | bundled | `misaki/data/us_gold.json` etc. read with `importlib.resources` (`en.py` line 143) |
| espeak-ng | bundled | from the `espeakng-loader` wheel |
| numba (librosa) | `NUMBA_CACHE_DIR` optional | librosa 0.11.0 has 30 `@jit(cache=True)` functions; cache files go next to the package (inside the venv) unless redirected |
| Telemetry | `HF_HUB_DISABLE_TELEMETRY=1`, `DO_NOT_TRACK=1` | gradio is never imported |

Remote endpoints that remain: `s3tokenizer` has modelscope.cn URLs in `load_model`, but chatterbox never calls it (S3 tokenizer weights come from `s3gen_meanflow.safetensors`).

## 9. VRAM measurement for both backends

| Method | Per-process on WDDM? | Notes |
|---|---|---|
| NVML `nvmlDeviceGetMemoryInfo` via `nvidia-ml-py` (official NVIDIA bindings, PyPI 13.615.71, import name `pynvml`) | No, device-wide | Loads `C:\Windows\System32\nvml.dll` (present on this machine). Sample used memory every 50-100 ms; report peak minus idle baseline. |
| NVML `usedGpuMemory` per process | No | NVML docs: "Under WDDM, NVML_VALUE_NOT_AVAILABLE is always reported because Windows KMD manages all the memory and not the NVIDIA driver." https://docs.nvidia.com/deploy/nvml-api/api/structnvmlProcessInfo__t.html . `nvidia-smi` shows `N/A` per process here (tested); `driver_model.current` = WDDM. |
| Windows counter `\GPU Process Memory(pid_<PID>_*)\Dedicated Usage` | Yes | tested: `typeperf "\GPU Process Memory(pid_<PID>_*)\Dedicated Usage" -sc 1` returned bytes per adapter LUID; filter to the NVIDIA LUID. Works for both the Python worker and `narrate-onnx`. |
| `torch.cuda.max_memory_allocated()` / `max_memory_reserved()` | PyTorch allocator only | not comparable with ORT |

`pynvml` on PyPI (13.0.1) is a deprecated wrapper whose README says the real bindings are `nvidia-ml-py`.

Since driver 536.40, NVIDIA's Windows driver lets CUDA apps "use shared memory in cases that exhausted the GPU memory", so they "continue to run, albeit at lower speeds"; the NVIDIA Control Panel setting "CUDA - Sysmem Fallback Policy" = "Prefer No Sysmem Fallback" disables it (https://nvidia.custhelp.com/app/answers/detail/a_id/5490). Over-budget runs on this driver (610.88) therefore slow down rather than fail by default; record shared usage (`\GPU Process Memory(...)\Shared Usage`) next to dedicated usage. This applies to ORT's CUDA EP as well (inferred: same driver path).

## Unverified

- Import time of `chatterbox` and model load time on this GPU.
- Turbo VRAM need (estimate from fp32 weights: about 3.0 GB plus activations).
- kokoro 0.9.4 running under transformers 5.2.0.
- Whether all 19 tags render well.
- uv's own SHA-256 check of the managed Python download.
- Chunk-size guidance in 1.6 (estimate).
