# Decisions

## 2026-10-12: Rust CLI with a managed Python runtime for Chatterbox-Turbo

Decision: `narrate` is a Rust CLI. Its `setup` command builds an isolated, version-pinned Python env with uv and runs the official chatterbox-tts package (Chatterbox-Turbo) locally, using CUDA when available. Downloads come only from official sources, pinned to exact versions and Hugging Face commit hashes.
Plan: .agent/main-plan/plan-narrate-cli-20261012-2003.md

## 2026-10-12: Experimental pure-Rust ONNX backend as a separate binary

Decision: After phase 1 works, `narrate-onnx` is added as a separate binary in the same repo. It runs the official `ResembleAI/chatterbox-turbo-ONNX` export through ONNX Runtime and is compared against `narrate`. `narrate` stays unchanged until the comparison is done.
Plan: .agent/main-plan/plan-narrate-cli-20261012-2003.md

## 2026-10-12: Voice presets

Decision: `--voice female` (default) clones a reference clip made once at setup with Kokoro-82M voice af_heart; `--voice male` clones Kokoro am_michael; `--voice builtin` uses Chatterbox-Turbo's bundled conds.pt voice; `--voice-file <wav>` accepts any custom reference clip.
Plan: .agent/main-plan/plan-narrate-cli-20261012-2003.md

## 2026-10-12: Inno Setup per-user installer

Decision: The Windows installer is an Inno Setup setup.exe that installs per user under %LOCALAPPDATA%\Programs, adds `narrate` to the user PATH, and needs no admin rights.
Plan: .agent/main-plan/plan-narrate-cli-20261012-2003.md

## 2026-10-12: Command name and push target

Decision: The command is `narrate`. Work is committed directly on main and pushed to origin main.
Plan: .agent/main-plan/plan-narrate-cli-20261012-2003.md

## 2026-10-12: Download source allow-list

Decision: Setup may fetch only from: PyPI through a committed, sha256-hashed lockfile; download.pytorch.org for torch-family packages only; uv's managed Python (astral-sh/python-build-standalone); the Hugging Face repos at pinned commits; and Kokoro's G2P assets (spaCy English model, espeak-ng) if needed. Every artifact is pinned and hash-verified.
Plan: .agent/main-plan/plan-narrate-cli-20261012-2003.md

## 2026-10-12: Watermark in narrate-onnx

Decision: narrate-onnx embeds the PerTh watermark only if an official ONNX or Rust port of PerTh exists; otherwise its output is unwatermarked and the backend comparison report states the difference.
Plan: .agent/main-plan/plan-narrate-cli-20261012-2003.md

## 2026-10-13: PyTorch wheel host

Decision: The allow-list adds `download-r2.pytorch.org`, the host the official PyTorch index uses for its wheel files, for torch and torchaudio only. `uv.lock` stays exactly as uv generates it, and sha256 checks still apply.
Plan: .agent/main-plan/plan-narrate-cli-20261012-2003.md

## 2026-10-13: Build backend for source-only packages

Decision: Packages that ship only as source (docopt 0.6.2) are built with the build backend constrained to the locked, hash-pinned `setuptools==81.0.0`. Setup fails if any other build dependency would be fetched.
Plan: .agent/main-plan/plan-narrate-cli-20261012-2003.md
