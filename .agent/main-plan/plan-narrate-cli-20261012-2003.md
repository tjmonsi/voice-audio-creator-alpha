# Execution Plan: narrate-cli

**Created:** 2026-10-12 20:03  
**Status:** Paused  
**User:** tjmonsi@gmail.com

## Summary

`narrate`, a Rust CLI with a per-user Inno Setup installer, turns a Markdown or text script into a WAV narration with the official chatterbox-tts Chatterbox-Turbo running locally on the GPU (phase 1). Phase 2 adds `narrate-onnx`, an experimental pure-Rust binary running the official ResembleAI ONNX export, and a report comparing the two backends.

## Steps

| # | Step | Agent | Model | Effort | Runs | Files |
|---|---|---|---|---|---|---|
| 1a | Write requirements for both phases | requirements-gatherer | Sonnet | medium | Wave 1, parallel with 1b | `docs/requirements.md` |
| 1b | Research the technical facts both phases depend on | researcher | Opus | xhigh | Wave 1, parallel with 1a | `docs/research/` |
| 2 | Write the phase-1 technical spec | solutions-architect | Opus | xhigh | After wave 1 | `specs/` |
| 3a | Implement `narrate` (Rust CLI, Python worker, setup, Inno script) | developer | Sonnet | high | Wave 3, parallel with 3b | `Cargo.toml`, `crates/`, `python/`, `installer/`, `README.md` |
| 3b | Write phase-1 black-box acceptance tests | developer-tester | Sonnet | high | Wave 3, parallel with 3a | test paths per spec |
| 4 | Run tests and coverage, run real `narrate setup`, GPU end-to-end with pitch check | qa | Sonnet | high | After wave 3 | `.agent/reports/` |
| 5 | Build installer, commit, push main, install, verify `narrate` on PATH | devops | Sonnet | high | After step 4 | `dist/`, git |
| 6 | Write the phase-2 delta spec for `narrate-onnx` | solutions-architect | Opus | xhigh | After step 5 | `specs/` |
| 7a | Implement `narrate-onnx` (ort + official ONNX export) | developer | Sonnet | high | Wave 7, parallel with 7b | `crates/narrate-onnx/` |
| 7b | Write phase-2 black-box acceptance tests | developer-tester | Sonnet | high | Wave 7, parallel with 7a | test paths per spec |
| 8 | Run tests, GPU end-to-end, benchmark both backends, write comparison | qa | Sonnet | high | After wave 7 | `docs/backend-comparison.md` |
| 9 | Rebuild installer with both binaries, commit, push main, reinstall, verify | devops | Sonnet | high | After step 8 | `dist/`, git |

Order: 1a and 1b in parallel, then 2, then 3a and 3b in parallel, then 4, then 5, then 6, then 7a and 7b in parallel, then 8, then 9.

## User Decisions

- Approved: 2026-10-12 20:25
- Preflight 2026-10-12 20:25: 40G free, ok
- Clarify 1, runtime: phase 1 is Rust CLI + managed Python (uv) running the official chatterbox-tts. Phase 2, after phase 1 works: a pure Rust + ONNX Runtime backend, compared against phase 1. Use official sources (official chatterbox-tts, official ResembleAI artifacts) to minimize security risk.
- Clarify 1, voices: both Kokoro-made presets and the Turbo built-in voice.
- Clarify 1, installer: Inno Setup, per-user install, adds to user PATH, no admin.
- Clarify 1, command name: `narrate`.
- Clarify 2, default voice: Kokoro female default. `--voice female` (default) = clone of Kokoro af_heart; `--voice male` = clone of Kokoro am_michael; `--voice builtin` = Turbo's bundled conds.pt voice; `--voice-file <wav>` for any custom clip.
- Clarify 2, phase-2 delivery: separate experimental binary `narrate-onnx` in the same repo; `narrate` stays untouched until the comparison is done.
- Clarify 2, push target: commit directly on main and push main.

- Gate 1a review 1 decisions (2026-10-12): source allow-list = PyPI via hash-locked lockfile, download.pytorch.org for torch only, uv python-build-standalone, HF repos at pinned commits, Kokoro G2P assets (spaCy model, espeak-ng) if needed; every artifact pinned and hash-verified.
- Gate 1a review 1 decisions (2026-10-12): narrate-onnx watermarks only if an official ONNX/Rust port of PerTh exists (research checks); otherwise output is unwatermarked and the comparison report states the difference.
- Gate 1a review 1 decisions (2026-10-12): builtin voice pitch is measured and reported only, no threshold.
- Stop (2026-10-13): user asked to show progress, commit and push the work so far, then stop. Interim commit and push to origin main done by devops before pausing; review gates not yet run on it.
- Step 3a decisions (2026-10-13): allow-list adds download-r2.pytorch.org for torch/torchaudio wheels only, uv.lock kept as generated; source-only packages (docopt 0.6.2) build with the backend constrained to locked setuptools==81.0.0, setup fails if any other build dependency is fetched.

## Assumptions

- Output is WAV at the model's sample rate. The default output path is the input path with a `.wav` extension; `-o/--output` overrides it.
- Markdown becomes spoken plain text: syntax is stripped, link text is read, code blocks are skipped, headings and paragraphs get pauses. A `.txt` file is read as-is. Turbo paralinguistic tags such as `[laugh]` pass through.
- Long scripts are split at sentence boundaries into model-sized chunks and joined with short silences.
- `--device auto|cuda|cpu`; `auto` picks CUDA when available and warns when falling back to CPU.
- Python env, model weights, and voice clips live in the per-user data dir (`%LOCALAPPDATA%\narrate`), shared by dev builds and the installed binary, so the step 4 setup also serves the installed command.
- Setup pins every package version and every Hugging Face model revision (commit hash), and downloads only from official sources: PyPI `chatterbox-tts`, the PyTorch CUDA wheel index, `ResembleAI/chatterbox-turbo`, `hexgrad/Kokoro-82M`, and (phase 2) `ResembleAI/chatterbox-turbo-ONNX`.
- The PerTh watermark that chatterbox-tts embeds stays on.
- No agent can listen to audio. Voice gender is checked by median pitch (F0); how good the voice sounds is your call after a listen.
- `narrate` is installed after phase 1 so you can use it right away, and reinstalled after phase 2 adds `narrate-onnx`.
- Inno Setup is installed on this machine with winget (`JRSoftware.InnoSetup`) to build the installer.
- `narrate-onnx` reuses the voice clips that `narrate setup` creates; phase 2 does not reimplement Kokoro.

---

## Step Details

### Step 1a: Write requirements for both phases

**Goal:** Produce BR/UR/FR/NFR requirements with GIVEN/WHEN/THEN acceptance criteria for `narrate` (phase 1) and `narrate-onnx` (phase 2).  
**Agent:** requirements-gatherer  
**Model:** Sonnet  
**Effort:** medium  
**Background:** yes  
**Effect:** write  
**Writes:** docs/requirements.md  
**Reads:** none  
**Relevant Files:**
- docs/requirements.md

**Details:**
- Skill: `gather-requirements` `document` subcommand, built from the user decisions and assumptions above
- Covers: CLI usage, setup command, voices, GPU use, Markdown and text handling, output, installer, offline local-only operation, official-source pinning, phase-2 binary and comparison
- Gate: Planning Document Review (reviewer, Opus)

**Review:** Planning gate: iterations 1-3 returned (25, 12, 4 findings), fix iterations 1-3 (requirements-gatherer, Sonnet); iteration 4 LGTM #1; iteration 5 LGTM #2. Exited: 2 consecutive LGTMs
**Status:** completed
**Done:** 2026-10-12: docs/requirements.md with BR/UR/FR/NFR IDs, 26 FRs, 12 NFRs, phases marked. Skipped markers: elicit, review, BRD/URD split, json output

### Step 1b: Research the technical facts both phases depend on

**Goal:** Record verified, cited facts the architect needs, since the architect has no web access.  
**Agent:** researcher  
**Model:** Opus  
**Effort:** xhigh  
**Background:** yes  
**Effect:** write  
**Writes:** docs/research/  
**Reads:** none  
**Relevant Files:**
- docs/research/

**Details:**
- chatterbox-tts 0.1.7: `ChatterboxTurboTTS` API, sample rate, behavior without `audio_prompt_path` (conds.pt), chunk length limits, paralinguistic tags, Python and torch pins
- PyTorch 2.6.0 CUDA wheels for Python 3.11 on Windows; uv commands to build a pinned env
- Kokoro-82M on Windows: package, espeak-ng needs, af_heart and am_michael voice IDs, version compatibility with chatterbox-tts pins
- Hugging Face revision pinning and current commit hashes for the three model repos
- `ResembleAI/chatterbox-turbo-ONNX`: graph inputs and outputs, generation loop, tokenizer, reference-audio preprocessing, precision variants
- Rust `ort` crate: version, CUDA execution provider on Windows, DLL distribution; `tokenizers` crate
- Inno Setup: per-user install and user PATH changes
- Gate: Report Quality (review-md)

**Review:** Report Quality (review-md): round 1, 1 finding (phase1-python-runtime.md:277 stale builtin threshold); fix round 1 (researcher, Sonnet); round 2 no issues. Approved
**Status:** completed
**Done:** 2026-10-12: docs/research/{README,phase1-python-runtime,phase2-onnx-runtime,installer}.md. Skipped markers: uv-lock-run, iscc-compile, model-runtime-tests
**Out of scope:** generate uv.lock (step 3a), compile Inno [Code] (steps 3a/5), runtime-test kokoro under transformers 5.2.0 and measure VRAM (step 4), ORT node placement (step 8)

### Step 2: Write the phase-1 technical spec

**Goal:** Turn the requirements and research into a phase-1 spec the developer and developer-tester can build from independently.  
**Agent:** solutions-architect  
**Model:** Opus  
**Effort:** xhigh  
**Background:** no  
**Effect:** write  
**Writes:** specs/  
**Reads:** docs/requirements.md, docs/research/  
**Relevant Files:**
- specs/

**Details:**
- Skill: `spec-from-requirements` `create`
- Cargo workspace layout ready for a second binary in phase 2
- CLI grammar, exit codes, error messages
- Public Rust API for text extraction and chunking, so black-box tests can target it
- Rust-to-Python worker protocol, data-dir layout, pinned setup procedure, installer behavior
- Test locations and required dev-dependencies
- Gate: Planning Document Review (reviewer, Opus)

**Status:** completed  
**Done:** 8 spec files under specs/ (architecture, cli, text-pipeline, generation, worker, setup, installer, test-strategy). Declared gaps: python-build-standalone URL and sha256 absent from research; uv archive-cache assumption untested; windows-sys version unpinned; several vendor-doc items for QA to confirm; kokoro 0.9.4 with transformers 5.2.0 untested at run time.  
**Out of scope:** .gitignore dist/ entry (goes to step 3a)  
**Review:** Planning Document Review gate: iteration 1 returned (5 must-fix, 4 should-fix, 7 low); fix pass 1 done (all 16 fixed; per-run temp dirs instead of clearing); iteration 2 returned (2 must-fix, 2 low, mostly from fix pass 1); fix pass 2 done (all 4 fixed); iteration 3 approved (LGTM 1); iteration 4 (fresh reviewer, full pass) returned (2 must-fix, 2 before step 4), LGTM count reset; fix pass 3 done (all 4 fixed); iteration 5 approved (LGTM 1); iteration 6 (full pass) approved (LGTM 2). Gate passed. Two low notes passed to steps 3a/3b: rule 6 inspects only source and artifact lines, never [package.metadata]; narrate-test-signal gets a spawn-group launcher for QA

### Step 3a: Implement `narrate`

**Goal:** Implement the phase-1 spec: Rust CLI, Python worker, `narrate setup`, and the Inno Setup script.  
**Agent:** developer  
**Model:** Sonnet  
**Effort:** high  
**Background:** yes  
**Effect:** write  
**Writes:** Cargo.toml, Cargo.lock, crates/ (excluding test paths), python/ (excluding test paths), installer/, README.md, .gitignore  
**Reads:** specs/, docs/  
**Relevant Files:**
- specs/
- Cargo.toml

**Details:**
- Skills: `task-plan`, then `code`; `technical-writing` for README.md
- Self-check: `cargo build`, `cargo clippy`; no tests written by this step
- Gates: Pre-Test Code Review (reviewer), then Security (reviewer); Testing Gate is step 4; Code Review sign-off (code-reviewer) runs after step 4 and before step 5

**Status:** pending  
**Done:** implemented; self-check=pass (cargo build, clippy -D warnings). Real setup and generation ran in a scratch data dir: setup 5m42s, offline rerun 2.3s; CUDA rtf 0.62-0.87; F0 female 199.3 Hz, male 122.0 Hz; watermark 1.000; Ctrl+Break exit 130. About 300 tests pass, 2 fail on a test bug (interrupt.rs:70). Installer not compiled (no Inno Setup).  
**Out of scope:** delta spec for: PyTorch r2 host and docopt build backend (user decided), lock rule 6 wording, worker stdin polling, Kokoro clips on CPU, builtin OOM retry on CPU, CLI parsing. Developer re-run follows the delta spec, then the Pre-Test gate.  
**Delta spec (2026-10-13):** written by solutions-architect in 5 specs (setup, worker, cli, architecture, test-strategy; Change Log 2026-10-13 sections). Planning gate on the delta not run (stopped).  
**Resume at:** (1) Planning gate on the delta spec; (2) developer: pyproject [tool.uv] keys, re-lock, two-run uv sync, verify uv 0.10.2 flags, cargo fmt on all files; developer-tester: FAKE_WORKER_FAIL_FIRST_LOAD knob and delta tests; (3) Pre-Test gate, Security gate, then step 4.

### Step 3b: Write phase-1 black-box acceptance tests

**Goal:** Write acceptance tests from the spec alone, each traced to its requirement or scenario ID.  
**Agent:** developer-tester  
**Model:** Sonnet  
**Effort:** high  
**Background:** yes  
**Effect:** write  
**Writes:** test paths named in specs/  
**Reads:** specs/, docs/requirements.md  
**Relevant Files:**
- specs/

**Details:**
- Skill: `create-test`
- No reading of implementation code
- Gate: Pre-Test Code Review findings on test files

**Status:** pending  
**Done:** 19 test files written (crates/narrate-core/tests, crates/narrate/tests, crates/narrate-testkit/tests/smoke.rs); coverage 25 of 30 source IDs, the 5 uncovered go to QA, devops and the owner's manual checks. Compile not run (no shell).  
**Out of scope:** developer-tester's write scope is test globs only, so the testkit package (manifest and 4 binaries) and the qa/ assets were not written. Reassigned within this step's approved writes to qa (Sonnet), black-box, working from the spec and smoke.rs/pins.rs contracts. qa wrote crates/narrate-testkit (manifest and 4 binaries) and 8 qa/ assets. smoke.rs:644 compile error fixed by developer-tester; smoke test target compiles (verified with cargo test --no-run).  
**Review:** gate interrupted by stop; joint Pre-Test gate pending

### Step 4: Run tests, setup, and GPU end-to-end

**Goal:** Prove phase 1 works on this machine and serve as the Testing Gate for steps 3a and 3b.  
**Agent:** qa  
**Model:** Sonnet  
**Effort:** high  
**Background:** no  
**Effect:** write  
**Writes:** .agent/reports/, %LOCALAPPDATA%\narrate (setup data)  
**Reads:** project  
**Relevant Files:**
- .agent/reports/

**Details:**
- Skill: `run-test`
- `cargo test`, coverage with cargo-llvm-cov (>=70% of new Rust code)
- Real `narrate setup` (downloads about 5 GB), then generation from a `.md` and a `.txt` script with `female`, `male`, and `builtin` voices on CUDA
- Median F0 check: female above 165 Hz, male below 155 Hz (thresholds per spec); builtin F0 measured and reported only
- Verdict `approved` or `returned`

**Status:** pending

### Step 5: Build installer, commit, push, install

**Goal:** Ship phase 1: setup.exe built, work committed on main and pushed, `narrate` installed and on PATH.  
**Agent:** devops  
**Model:** Sonnet  
**Effort:** high  
**Background:** no  
**Effect:** write  
**Writes:** dist/ (gitignored), git history on main, origin/main, user install dir, user PATH, Inno Setup install via winget  
**Reads:** project  
**Relevant Files:**
- installer/

**Details:**
- `cargo build --release`, build setup.exe with ISCC
- Conventional commits on main with the required Co-Authored-By trailer; push origin main
- Run setup.exe silently; verify `narrate --version` and setup status from a new shell
- Leaves `.agent/` uncommitted unless the spec says otherwise

**Status:** pending

### Step 6: Write the phase-2 delta spec

**Goal:** Specify `narrate-onnx` against the official ONNX export and define the comparison method.  
**Agent:** solutions-architect  
**Model:** Opus  
**Effort:** xhigh  
**Background:** no  
**Effect:** write  
**Writes:** specs/  
**Reads:** docs/, specs/, crates/, .agent/reports/  
**Relevant Files:**
- specs/

**Details:**
- Skill: `spec-from-requirements` `delta`
- Pinned `ResembleAI/chatterbox-turbo-ONNX` revision, precision variant choice, `ort` CUDA provider, reuse of setup voice clips
- Comparison metrics: real-time factor, load time, peak VRAM, disk size, output pitch and duration, dependency surface
- Gate: Planning Document Review (reviewer, Opus)

**Status:** pending

### Step 7a: Implement `narrate-onnx`

**Goal:** Implement the phase-2 delta spec as a separate binary.  
**Agent:** developer  
**Model:** Sonnet  
**Effort:** high  
**Background:** yes  
**Effect:** write  
**Writes:** crates/narrate-onnx/ (excluding test paths), Cargo.toml, Cargo.lock, installer/, README.md  
**Reads:** specs/, crates/  
**Relevant Files:**
- crates/narrate-onnx/

**Details:**
- Skills: `task-plan`, `code`
- Gates: Pre-Test (reviewer), Security (reviewer); Testing Gate is step 8; Code Review sign-off before step 9

**Status:** pending

### Step 7b: Write phase-2 black-box acceptance tests

**Goal:** Write acceptance tests for `narrate-onnx` from the delta spec alone.  
**Agent:** developer-tester  
**Model:** Sonnet  
**Effort:** high  
**Background:** yes  
**Effect:** write  
**Writes:** phase-2 test paths named in specs/  
**Reads:** specs/  
**Relevant Files:**
- specs/

**Details:**
- Skill: `create-test`

**Status:** pending

### Step 8: Test phase 2 and compare backends

**Goal:** Prove `narrate-onnx` works on the GPU and compare it with `narrate`.  
**Agent:** qa  
**Model:** Sonnet  
**Effort:** high  
**Background:** no  
**Effect:** write  
**Writes:** docs/backend-comparison.md, .agent/reports/, %LOCALAPPDATA%\narrate (ONNX model data)  
**Reads:** project  
**Relevant Files:**
- docs/backend-comparison.md

**Details:**
- Skill: `run-test`
- Same scripts and voices on both backends; metrics per the delta spec
- Gates: Testing Gate verdict; Report Quality (review-md) on docs/backend-comparison.md

**Status:** pending

### Step 9: Ship phase 2

**Goal:** Installer carries both binaries; work committed on main, pushed, reinstalled, and verified.  
**Agent:** devops  
**Model:** Sonnet  
**Effort:** high  
**Background:** no  
**Effect:** write  
**Writes:** dist/, git history on main, origin/main, user install dir  
**Reads:** project  
**Relevant Files:**
- installer/

**Details:**
- Same procedure as step 5; verify `narrate` and `narrate-onnx` from a new shell

**Status:** pending

---

## Review Gates

**Planning Document Review (steps 1a, 2, 6):** Pending  
**Report Quality (steps 1b, 8):** Pending  
**Pre-Test, Security (OWASP) (steps 3a/3b, 7a/7b):** Pending  
**Testing (>=70% coverage) (steps 4, 8):** Pending  
**Code Review (sign-off) (before steps 5, 9):** Pending

---

## Success Criteria

- [ ] All steps completed
- [ ] All review gates passed
- [ ] `narrate script.md` produces a WAV on the GPU from an installed command
- [ ] User approved final result
