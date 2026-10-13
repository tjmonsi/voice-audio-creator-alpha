# spec-narrate-test-strategy

## Metadata

| Field | Value |
|---|---|
| **Feature** | Phase 1 test strategy: test locations, dev-dependencies, test kit, fabricated data directory, coverage map, GPU QA procedure, installer checks, manual Owner checks |
| **Parent feature** | spec-narrate-architecture.md |
| **Sub-features** | None |
| **Requirement refs** | FR-001, FR-002, FR-003, FR-004, FR-005, FR-006, FR-007, FR-008, FR-009, FR-010, FR-011, FR-012, FR-013, FR-014, FR-015, FR-016, FR-017, FR-018, FR-024, NFR-001, NFR-002, NFR-003, NFR-004, NFR-005, NFR-006, NFR-007, NFR-008, NFR-009, NFR-011, NFR-012 |
| **Feature type** | Tooling |
| **Status** | Review |

Status values: Draft, Review, Approved, Implemented.

Change Log 2026-10-13 (end of this file) modifies the test kit's proxy host list, the repository-inspection assertions, and the automated setup-step cases, and adds GPU QA item 14. Where the change log and the body differ, the change log wins.

## Description

Phase 1 is verified at three levels. Black-box Rust tests (written by the developer-tester from the specs alone) cover the library API and every CLI behavior that does not need the model, using a fabricated data directory and a fake worker, fake uv, allow-list proxy, and console-signal helper from the `narrate-testkit` crate; they need no GPU, network, uv, or Python. GPU QA (plan step 4) runs the real setup and real generation on the RTX 3050 and executes the QA assets under `qa/`. devops (plan step 5) runs the installer checks, and the Owner performs the few checks no agent can (listening, a real Ctrl+C keypress, the double-click install).

## Triggered by

| Trigger type | Source | Condition |
|---|---|---|
| Plan step 3b | developer-tester | Writes every test path below |
| Plan step 4 | qa | Runs `cargo test`, coverage, and the GPU QA procedure |
| Plan step 5 | devops | Runs the installer checks |
| Manual | Owner | Manual checks after step 5 |

## Required data

| Data | Source | Type | Notes |
|---|---|---|---|
| Specs | `specs/spec-narrate-*.md` | Markdown | Only source for test expectations |
| `python/uv.lock`, `python/pyproject.toml`, `crates/narrate/pins.json` | Repository | Files | Read by repository-inspection tests |
| Real data directory | `%LOCALAPPDATA%\narrate` | Directory | GPU QA only; automated tests never touch it |

## Behavior

### Requirement: Test locations and ownership

The developer-tester SHALL write tests only at the paths below, and the developer SHALL write none of them; the developer SHALL provide the manifest entries listed under "Dev-dependencies" so the tests compile.

| Path | Owner | Contents |
|---|---|---|
| `crates/narrate-core/tests/script.rs` | developer-tester | `ScriptKind`, `decode_utf8`, `load_script`, `ScriptError` (FR-001, FR-004) |
| `crates/narrate-core/tests/markdown.rs` | developer-tester | Markdown rules and the worked examples (FR-002) |
| `crates/narrate-core/tests/tags.rs` | developer-tester | Tag pass-through, `PARALINGUISTIC_TAGS`, `unknown_tags` (FR-003) |
| `crates/narrate-core/tests/chunk.rs` | developer-tester | `split_sentences`, `chunk_script`, pauses (FR-005) |
| `crates/narrate-core/tests/audio.rs` | developer-tester | `f32_to_pcm16`, `wav_file_size`, `write_wav_pcm16` header bytes (FR-006) |
| `crates/narrate-core/tests/clip.rs` | developer-tester | `inspect_clip`, `is_too_short`, `needs_trim`, `write_clip_prefix` (FR-009) |
| `crates/narrate/tests/common/mod.rs` | developer-tester | Shared helpers (binary paths, fabricated data directory, pin id lookup) |
| `crates/narrate/tests/cli_errors.rs` | developer-tester | Usage errors, help, version, error format, verbose details, no watermark option (FR-024, FR-018, NFR-007, NFR-008) |
| `crates/narrate/tests/input.rs` | developer-tester | Input checks and text sent to the worker (FR-001 to FR-004) |
| `crates/narrate/tests/output.rs` | developer-tester | Output path rules, overwrite, permission denied, size limit (FR-006, NFR-005) |
| `crates/narrate/tests/voices.rs` | developer-tester | Voice lines, voice requests, custom clip rules (FR-007, FR-008, FR-009) |
| `crates/narrate/tests/device.rs` | developer-tester | Device lines, fallback warning, CUDA error, watermark hook (FR-013, FR-018) |
| `crates/narrate/tests/chunks.rs` | developer-tester | Verbose sample sums, pauses, failure hooks, seeds (FR-005) |
| `crates/narrate/tests/interrupt.rs` | developer-tester | Ctrl+Break and killed parent (FR-005) |
| `crates/narrate/tests/status.rs` | developer-tester | Status report, readiness, override, pin mismatch (FR-012, FR-014, FR-015, FR-017) |
| `crates/narrate/tests/setup_preflight.rs` | developer-tester | uv missing, uv version, disallowed source, unhashed artifact, torch index, other package from the PyTorch index, unrecognized lock line, unsafe pin path (FR-010, FR-011) |
| `crates/narrate/tests/setup_steps.rs` | developer-tester | Setup steps 2 to 6 and setup interruption with the fake uv and fake worker (Requirement: Automated setup-step tests) (FR-010, NFR-001, NFR-002) |
| `crates/narrate/tests/pins.rs` | developer-tester | Repository inspection of pins and lock (FR-011, NFR-001) |
| `crates/narrate-testkit/` (whole package) | developer-tester | Test-only binaries (Requirement: Test kit) |
| `qa/` | developer-tester | GPU QA and installer assets (Requirement: QA assets) |

Python worker unit tests (`python/tests/`) are not written in Phase 1: `pytest` is not in the pinned lock (adding it would change the pin set and the Owner's environment), the worker is thin glue around the model, and GPU QA exercises it end to end through `narrate`.

#### Scenario: Tests compile against the developer's manifests

- **GIVEN** the developer's workspace and the tester's files merged
- **WHEN** `cargo test --workspace --all-features --no-run` runs
- **THEN** it SHALL succeed

### Requirement: Test command and dev-dependencies

Automated tests SHALL run with `cargo test --workspace --all-features` (the `test-hooks` feature of `narrate` must be on, and `--workspace` builds the test-kit binaries); `crates/narrate/Cargo.toml` SHALL list `[dev-dependencies]` `assert_cmd` `=2.2.2`, `predicates` `=3.1.4`, `tempfile` `=3.27.0`, `hound` `=3.5.1`, `serde_json` `=1.0.154` (all `workspace = true`), and `crates/narrate-core/Cargo.toml` SHALL list `tempfile` `=3.27.0` as a dev-dependency (research P1 section 6).

Rules for every automated test: give each test its own fresh `tempfile` data directory and set `NARRATE_DATA_DIR` to it for every `narrate` invocation in that test (a test may run several invocations against the same directory, for example the fabricated-directory recipe or a setup re-run); never use network, GPU, uv, or Python; remove any deny ACE a test adds before the temporary directory is dropped (`icacls <dir> /remove:d "%USERNAME%"`).

#### Scenario: Suite passes without GPU or network

- **GIVEN** a machine with the network unplugged, no uv on PATH, and no data directory
- **WHEN** `cargo test --workspace --all-features` runs
- **THEN** every test SHALL pass

#### Scenario: Missing test-kit binary is reported

- **GIVEN** a run of `cargo test -p narrate` (without `--workspace`) after a clean build
- **WHEN** a test needs `narrate-test-worker.exe`
- **THEN** the helper SHALL panic with a message naming `cargo test --workspace --all-features`

### Requirement: Test kit

The package `crates/narrate-testkit` SHALL be `publish = false`, depend only on workspace dependencies (`serde`, `serde_json`, `windows-sys`), contain the four binaries below and an integration test `tests/smoke.rs` (so Cargo builds the binaries during `cargo test --workspace`), and the `narrate` tests SHALL locate each binary as a sibling of `env!("CARGO_BIN_EXE_narrate")`.

| Binary | Purpose | Contract |
|---|---|---|
| `narrate-test-worker` | Fake worker | spec-narrate-worker.md Requirement 9 and Protocol schemas; configuration through `FAKE_WORKER_*` variables chosen by the tester |
| `narrate-test-uv` | Fake uv | Every invocation appends its arguments as one line to `FAKE_UV_LOG` when set. `--version` prints `uv <FAKE_UV_VERSION, default 0.10.2> (test)` and exits 0. `python install ...`: with `--offline`, exits 0 only if `UV_PYTHON_CACHE_DIR` already holds a copy of the file named by `FAKE_UV_PY_ARCHIVE`, else exits 1; without `--offline`, creates `UV_PYTHON_CACHE_DIR` and `UV_PYTHON_INSTALL_DIR`, copies that file into `UV_PYTHON_CACHE_DIR`, and exits 0. `sync ...`: sleeps `FAKE_UV_SYNC_SLEEP_MS` (default 0); if `FAKE_UV_SYNC_EXIT` is set, prints `FAKE_UV_SYNC_STDERR` to stderr and exits with that code; with `--offline`, exits 0 only if the venv marker files already exist under `UV_PROJECT_ENVIRONMENT`, else exits 1; without `--offline`, creates the marker files (`pyvenv.cfg`, `Scripts\python.exe`, `Lib\site-packages\chatterbox_tts-0.1.7.dist-info\METADATA`, all empty) and exits 0. Any other invocation exits `FAKE_UV_EXIT` (default 1) |
| `narrate-test-proxy` | Allow-list HTTPS proxy for GPU QA | `--listen 127.0.0.1:<port> --log <file>`; tunnels `CONNECT host:port` when the host is in the allow-list of spec-narrate-setup.md (exact hosts plus suffixes `.hf.co`, `.huggingface.co`), else answers 403; rejects non-CONNECT requests with 403; appends `<unix ms>\t<ALLOW or DENY>\t<host>:<port>` per request |
| `narrate-test-signal` | Console signal and process helper | `narrate-test-signal <process-group-id>` calls `GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, <id>)` and exits 0 on success, 1 on failure. `narrate-test-signal alive <pid>` opens the process with `PROCESS_QUERY_LIMITED_INFORMATION` and exits 0 if it exists and its exit code is still `STILL_ACTIVE` (259), else 1 (this is how tests confirm a worker pid is gone, since `narrate`'s dev-dependencies have no Win32 access) |

Interrupt tests start `narrate` with `std::process::Command` and `CREATE_NEW_PROCESS_GROUP` (`0x00000200` through `std::os::windows::process::CommandExt::creation_flags`), so its process id is the group id. `narrate` itself starts the worker and uv in their own process groups without a console (spec-narrate-setup.md, Child process environment), so the event reaches only `narrate`.

The test kit's Win32 calls use `windows-sys` `=0.61.2` (spec-narrate-architecture.md); test code SHALL NOT name the `BOOL` or `HANDLE` types (let type inference bind them and compare `BOOL` results with `!= 0`), because their module paths differ between `windows-sys` releases.

#### Scenario: Smoke test

- **GIVEN** the test kit
- **WHEN** `tests/smoke.rs` runs `narrate-test-worker` and sends `{"id":1,"op":"hello","protocol":1}`
- **THEN** the reply SHALL be a JSON object with `id` 1, `ok` true, and `protocol` 1

### Requirement: Fabricated ready data directory

CLI tests that need a ready data directory SHALL build one in a temporary directory `D` by (1) creating, as empty files, every path in the readiness table of spec-narrate-setup.md (Requirement 7); (2) running `narrate status` with `NARRATE_DATA_DIR` set to a second, empty temporary directory and reading the 64-hex id from the line `pin set: missing (binary <id>, recorded none)`; (3) writing `D\setup.json` as `{"format":1,"pin_set_id":"<id>"}`; and SHALL run `narrate` with `NARRATE_DATA_DIR=D` and `NARRATE_WORKER_CMD=<path of narrate-test-worker.exe>`.

#### Scenario: Fabricated directory accepted

- **GIVEN** a directory built by the recipe
- **WHEN** `narrate status` runs with the fake worker
- **THEN** it SHALL exit 0 with `status: ready`

### Requirement: Automated setup-step tests

`crates/narrate/tests/setup_steps.rs` SHALL exercise setup steps 2 to 6 and setup interruption without network or Python by running `narrate setup` (test build) with `NARRATE_UV` = `narrate-test-uv`, `NARRATE_WORKER_CMD` = `narrate-test-worker`, an empty per-test data directory, and `NARRATE_TEST_PINS_FILE` naming a test pins file the test writes at run time: same schema and allowed repos and revisions as `crates/narrate/pins.json`, the same 13 file paths (so the readiness check finds them), each with the size and sha256 of a small fixture file the test creates and the fake worker serves through `FAKE_WORKER_DOWNLOAD_DIR`, and `python.sha256` = the sha256 of the fixture named by `FAKE_UV_PY_ARCHIVE`. The embedded `uv.lock` stays in effect (it passes preflight). Every `narrate` invocation within a case (each setup run, each re-run, and every `narrate status` check) SHALL set the same `NARRATE_TEST_PINS_FILE`, `NARRATE_UV`, `NARRATE_WORKER_CMD`, and `NARRATE_DATA_DIR`, because the pin set id is computed from the effective `pins.json`; a `status` without the test pins file reports `pin set: mismatch`. The cases are the "(automated)" scenarios of spec-narrate-setup.md: python step with the fake uv; archive hash mismatch; model hash mismatch with the fake worker; worker path outside staging; one clip deleted; record written last; first run and re-run with the fakes; interrupted environment step. Setup runs need `FAKE_WORKER_SAMPLES_PER_CHAR` high enough that the builtin clip's `synth` returns at least 240000 samples: the reference text of spec-narrate-setup.md is 223 characters, so at least 1077 (1077 x 223 = 240171). One extra case sets it so only the doubled reference text (447 characters: two copies joined by one space) reaches 240000 samples, which holds for any value from 537 to 1076 (537 x 447 = 240039; 1076 x 223 = 239948), and asserts that `builtin.wav` is written with at least 240000 samples.

#### Scenario: Setup steps covered without network

- **GIVEN** the fakes and fixtures above
- **WHEN** `cargo test --workspace --all-features --test setup_steps` runs on a machine without network, uv, or Python
- **THEN** every case SHALL pass

### Requirement: Repository inspection

`crates/narrate/tests/pins.rs` SHALL read the committed files with paths relative to `env!("CARGO_MANIFEST_DIR")` and assert: `pins.json` has `uv.version` `0.10.2`; `python.version` `3.11.14`; `python.build` `cpython-3.11.14-windows-x86_64-none`; `python.url` exactly `https://github.com/astral-sh/python-build-standalone/releases/download/20260203/cpython-3.11.14%2B20260203-x86_64-pc-windows-msvc-install_only_stripped.tar.gz`; `python.sha256` exactly `851ba0c5a7c281912ca9bf68c340c69156e9e292de26f9068137f5650b537ff5`; the two repos with revisions `749d1c1a46eb10492095d68fbcf55691ccf137cd` and `f3ff3571791e39611d31c381e3a41a3af07b4987`; exactly the 13 files, with the paths, byte sizes, and full sha256 values of the "Pinned model files" table in spec-narrate-setup.md (Pin file schema), under the repo each row names; the reference text of spec-narrate-setup.md. `python/uv.lock` lists `chatterbox-tts` 0.1.7, `torch` `2.6.0+cu124` and `torchaudio` `2.6.0+cu124` from `download.pytorch.org`, `setuptools` 81.0.0, `en-core-web-sm` from the spaCy GitHub URL with sha256 `1932429db727d4bff3deed6b34cfc05df17794f4a52eeb26cf8928f7c1a0fb85`, and every `sdist` or `wheels` entry has a `sha256:` hash. `python/pyproject.toml` declares the `pytorch-cu124` index with `explicit = true` (FR-011, NFR-001).

#### Scenario: Pins recorded (FR-011)

- **GIVEN** the repository source
- **WHEN** `pins.rs` runs
- **THEN** every assertion above SHALL hold

### Requirement: QA assets

The developer-tester SHALL provide these files for GPU QA; Python scripts run with `<D>\venv\Scripts\python.exe -I <script> <args>` and `NUMBA_CACHE_DIR=<D>\numba-cache` so they use only the pinned environment and write nothing outside the data directory.

| Path | Contents |
|---|---|
| `qa/pitch/pitch-script.txt` | Exactly: `Every evening, the old lighthouse keeper climbed the spiral stairs to light the lamp. From the top, he could see the whole bay, the small village, and the hills beyond. He wrote down the weather in a worn notebook, as his father had done before him. On clear nights, the stars seemed close enough to touch.` followed by one newline (56 words) |
| `qa/pitch/median_f0.py` | For each WAV argument: `librosa.load(path, sr=16000, mono=True)`; `librosa.pyin(y, fmin=65.0, fmax=500.0, sr=16000, frame_length=1024)` (default hop 256); voiced frames = frames where `voiced_flag` is true and `f0` is finite; exit 2 if fewer than 100 voiced frames; median = `numpy.median` of `f0` over voiced frames; print `<path>\tmedian_f0_hz=<repr of the unrounded float>\tvoiced_frames=<n>\tframes=<m>` (research P1 section 5). The value is printed unrounded so threshold comparisons use the exact median |
| `qa/watermark/check_watermark.py` | For each WAV argument: load at the native rate (`librosa.load(path, sr=None)`), run `perth.PerthImplicitWatermarker().get_watermark(audio, sample_rate=sr)`, print `<path>\twatermark=<x.3>`. This extractor call follows the chatterbox README, not the research notes; QA confirms it runs before relying on it |
| `qa/long/script-10000-words.txt` | The pitch script paragraph repeated 179 times, separated by blank lines (10,024 words) |
| `qa/nfr002/snapshot.ps1` | `-Out <file>` records, for `%LOCALAPPDATA%\uv`, `%APPDATA%\uv`, `%USERPROFILE%\.local\bin`, `%USERPROFILE%\.cache\huggingface`, `%USERPROFILE%\.cache\torch`: existence and a recursive list of path, size, last-write time; for `HKCU\Software\Python`: existence and all subkeys and values; the user `Path` value; and the names of top-level entries of `%USERPROFILE%`, `%APPDATA%`, `%LOCALAPPDATA%`, and `%TEMP%`. `-Compare <before> <after>` prints differences and exits 1 when any listed location, the registry key, or the user `Path` differs; new top-level entries (including in `%TEMP%`) are printed for QA to attribute, since other programs also write there, and `-Compare` also exits 1 when a new `%TEMP%` entry contains a file whose name contains `espeak` (the phonemizer leftover that `TEMP`/`TMP` redirection prevents) |
| `qa/offline/watch_connections.ps1` | `-ProcessId <pid> -Out <file>`: every 500 ms until the process exits, records `Get-NetTCPConnection -OwningProcess <pid>` rows whose remote address is not `127.0.0.1`, `::1`, `0.0.0.0`, or `::`; exits 1 if any were recorded |
| `qa/vram/sample_vram.ps1` | `-Out <file>`: runs `nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits -lms 200` (NVML) and writes timestamped samples until stopped |
| `qa/installer/check_installer.ps1` | The installer check sequence below, printing PASS or FAIL per check |

#### Scenario: Pitch script fixed

- **GIVEN** `qa/pitch/pitch-script.txt`
- **WHEN** its content is compared with the text above
- **THEN** it SHALL match exactly

### Requirement: GPU QA procedure (plan step 4)

QA SHALL run the procedure below on the RTX 3050 after closing other GPU programs and recording the idle VRAM baseline (research F8), using the `test-hooks` debug build for hook runs and the release build for timing, and SHALL record every measured value with the commit, versions, and VRAM baseline in the QA report.

1. Automated: `cargo test --workspace --all-features`; coverage with `cargo llvm-cov --workspace --all-features --ignore-filename-regex narrate-testkit`. The target is per crate: at least 70% line coverage of `crates/narrate/src` and, separately, at least 70% of `crates/narrate-core/src` (plan step 4); the Python worker is not Rust and is outside this measure.
2. Scratch setup with `NARRATE_DATA_DIR=<scratch>`, `HTTP_PROXY` and `HTTPS_PROXY` = the allow-list proxy, and `qa/nfr002/snapshot.ps1` before the first run:
   0. `NARRATE_TEST_PINS_FILE` = pins copy with `python.sha256` altered: expect exit 5, `error[hash-mismatch]` naming the python-build-standalone URL, and `<scratch>\python` and `<scratch>\python-cache` absent afterwards (NFR-001).
   1. `NARRATE_TEST_LOCK_FILE` = lock copy with one small wheel's sha256 altered: expect exit 5, `error[hash-mismatch]` naming the package (NFR-001).
   2. `NARRATE_TEST_PINS_FILE` = pins copy with `added_tokens.json` sha256 altered: expect exit 5, `error[hash-mismatch]` naming `ResembleAI/chatterbox-turbo/added_tokens.json` (NFR-001).
   3. Real pins: first delete `<scratch>\models\chatterbox-turbo\t3_turbo_v1.safetensors` (item 2.2 may already have moved it into place, because every file that passes verification before the `added_tokens.json` mismatch is renamed into its final path); then start setup and stop the proxy while `t3_turbo_v1.safetensors` downloads (watch `<scratch>\staging\models\chatterbox-turbo\` grow); expect exit 5 naming `models`; `narrate status` exits 4; restart the proxy and re-run; expect `setup: ready` and `narrate status` exit 0 (FR-010).
   4. Proxy log: every line `ALLOW`, no `DENY` (FR-011).
   5. Record the clips' sha256; re-run setup with `HTTP_PROXY` and `HTTPS_PROXY` = `http://127.0.0.1:9`; expect every step `no network used` or `all present`, exit 0, and unchanged clip hashes (FR-010).
   6. `snapshot.ps1 -Compare`: exit 0 and the uv cache, Python installs, and Hugging Face cache present inside the scratch directory (NFR-002).
   7. Each clip is 24000 Hz, mono, 16-bit, at least 240000 samples (FR-010).
3. Default data directory: run `narrate setup` (or move the completed scratch directory to `%LOCALAPPDATA%\narrate` and run `narrate setup`, which repairs the venv without network). A debug build of the same commit then reports `status: ready` (FR-017).
4. Generation on CUDA (release build) of a `.md` and a `.txt` script with `female`, `male`, and `builtin`: each exits 0, prints `device: cuda (...)`, and the `done:` line's RTF is recorded (NFR-006, NFR-012). Record seconds of audio per character to check the 300-character chunk estimate.
5. Pitch: generate `qa/pitch/pitch-script.txt` with each voice on CUDA; the unrounded `median_f0_hz` from `median_f0.py` must be >= 165.0 Hz for female and <= 155.0 Hz for male (164.96 fails; no rounding before the comparison); builtin is reported only (NFR-004). Also run `median_f0.py` on the reference clips `<D>\voices\female.wav` and `<D>\voices\male.wav` themselves: each must have at least 100 voiced frames, female >= 165.0 Hz and male <= 155.0 Hz (unrounded), which catches a Kokoro model loaded with mismatched weights (kokoro's `strict=False` retry) before it shapes every cloned voice. Only the Owner may change these thresholds; QA reports a miss and does not adjust them.
6. Watermark: `check_watermark.py` gives >= 0.5 on every narrate output and < 0.5 on `<D>\voices\female.wav` (FR-018).
7. Offline: with `HTTP_PROXY` and `HTTPS_PROXY` = `http://127.0.0.1:9`, run with `--verbose`, start `watch_connections.ps1` on the worker pid from the `verbose: worker pid` line; expect exit 0 and no remote connection (FR-012).
8. Device: `CUDA_VISIBLE_DEVICES=-1` with `--device auto` prints the fallback warning and `device: cpu` and succeeds; with `--device cuda` exits 1 with `error[cuda-unavailable]` (FR-013).
9. Hooks with the real worker (test-hooks build): `NARRATE_TEST_FAIL_CHUNK=3` on a 5-chunk script exits 1 naming chunk 3 of 5 with no output file; `NARRATE_TEST_WATERMARK_FAIL=1` exits 1 with `error[watermark-unavailable]` and no output file (FR-005, FR-018).
10. Interrupt: with `sample_vram.ps1` running, start `narrate --verbose` in a new process group, send Ctrl+Break (`narrate-test-signal <narrate pid>`) during chunk 2, then in a second run terminate `narrate`; each time `narrate-test-signal alive <worker pid>` (pid from the `verbose: worker pid` line) exits 1 and device memory is within 256 MiB of the baseline within 10 s, the first run exits 130 with `error[interrupted]` (not `worker-exited`), and no output or `.tmp` file remains (FR-005).
11. Long script: `qa/long/script-10000-words.txt` with `female` on CUDA completes with exit 0, no `gpu-out-of-memory`; record peak device memory over baseline, the process's dedicated and shared GPU memory (`\GPU Process Memory(pid_<PID>_*)\Dedicated Usage` and `Shared Usage`, research P1 section 9), duration, and RTF (NFR-005, NFR-006, NFR-012).
12. Custom clip: `--voice-file` with a clip longer than 10 s prints the notice and succeeds (FR-009).
13. Release build ignores hooks: `target\release\narrate.exe --version` prints exactly `narrate 0.1.0`; `narrate status` with `NARRATE_WORKER_CMD=C:\does-not-exist.exe` still prints `cuda: available (...)` from the real worker; generation with `NARRATE_TEST_FAIL_CHUNK=1` succeeds (spec-narrate-cli.md, Requirement: Test hooks honored only in test builds).

#### Scenario: QA verdict

- **GIVEN** the procedure above
- **WHEN** QA finishes
- **THEN** the verdict SHALL be `approved` only if every item passed, and the report SHALL list each item with its evidence

### Requirement: Installer checks (plan step 5)

devops SHALL run, in order: (1) `installer\build.ps1`; (2) the deny-ACL install into a scratch `/DIR` (expect non-zero exit and no PATH entry; then remove the ACE); (3) silent install (exit 0, files under `%LOCALAPPDATA%\Programs\narrate`, nothing under `C:\Program Files`); (4) user PATH contains the install directory exactly once and a process with PATH rebuilt from the registry prints `narrate 0.1.0`; (5) silent reinstall and PATH still exactly once; (6) silent uninstall: install directory gone, PATH entry gone, other PATH entries unchanged, `%LOCALAPPDATA%\narrate` kept; (7) silent install again and `narrate status` from a rebuilt-PATH process exits 0 (FR-016, NFR-003, NFR-009).

#### Scenario: Installer checks pass

- **GIVEN** a built installer
- **WHEN** `qa/installer/check_installer.ps1` runs
- **THEN** every check SHALL print PASS and the Owner SHALL end with `narrate` installed

### Requirement: Manual Owner checks

The Owner SHALL perform these checks, which no agent can: (1) listen to the default female output of a real script and accept or reject the voice quality (NFR-011); (2) press Ctrl+C in a terminal during a generation and confirm the command ends within 10 s with exit code 130 and no output or `.tmp` file (FR-005); (3) double-click `narrate-setup-0.1.0.exe` and confirm no UAC prompt appears (FR-016).

#### Scenario: Owner acceptance

- **GIVEN** an installed `narrate` and a completed setup
- **WHEN** the Owner runs the three checks
- **THEN** each result SHALL be recorded as accepted or rejected in the plan's user decisions

## Coverage map

| Requirement | Automated (no GPU) | GPU QA item | Installer or manual |
|---|---|---|---|
| FR-001 | `script.rs`, `input.rs` | 4 | |
| FR-002 | `markdown.rs`, `input.rs`, `chunks.rs` (pause samples) | 4 | |
| FR-003 | `tags.rs`, `input.rs` | | |
| FR-004 | `script.rs`, `input.rs` | | |
| FR-005 | `chunk.rs`, `chunks.rs`, `interrupt.rs` | 9, 10 | Manual 2 |
| FR-006 | `audio.rs`, `output.rs` | 4 | |
| FR-007 | `voices.rs` | 4, 5 | |
| FR-008 | `voices.rs` (line, conds.pt readiness) | 4, 5 | |
| FR-009 | `clip.rs`, `voices.rs` | 12 | |
| FR-010 | `setup_preflight.rs` (uv missing), `setup_steps.rs` (steps 2 to 6, re-run lines, deleted clip, record last, interruption) | 2.3, 2.5, 2.7 | |
| FR-011 | `setup_preflight.rs` (allow-list, lock scanner, pin rules), `pins.rs` | 2.4 | |
| FR-012 | `status.rs` (readiness), `chunks.rs` (worker env offline) | 7 | |
| FR-013 | `device.rs` | 4, 8 | |
| FR-014 | `status.rs` | 2.3, 3 | |
| FR-015 | `status.rs`, `input.rs` | | |
| FR-016 | | | Installer checks; Manual 3 |
| FR-017 | `status.rs` | 3 | |
| FR-018 | `cli_errors.rs` (no option), `device.rs` (hook) | 6, 9 | |
| FR-024 | `cli_errors.rs` | | |
| NFR-001 | `pins.rs`, `setup_steps.rs` (archive and model hash mismatch) | 2.0, 2.1, 2.2 | |
| NFR-002 | `chunks.rs` (worker env has no inherited `UV_`; `TEMP` and `TMP` under `D\tmp\`) | 2.6 (including the `%TEMP%` check) | |
| NFR-003 | | 2 (setup as the Owner's standard user) | Installer checks |
| NFR-004 | | 5 (outputs and reference clips, unrounded comparison) | |
| NFR-005 | `output.rs` (limit hook) | 11 | |
| NFR-006 | | 4, 11 | |
| NFR-007 | every error test asserts the `  fix:` line; `cli_errors.rs` (detail lines) | | |
| NFR-008 | every test asserts the exit code | | |
| NFR-009 | | all | Installer checks |
| NFR-011 | | | Manual 1 |
| NFR-012 | | 4, 11 | |

## Algorithmic steps

1. developer-tester writes the test paths and QA assets (step 3b) in parallel with the developer (step 3a).
2. QA runs the GPU QA procedure (step 4) and issues the verdict.
3. devops runs the installer checks (step 5).
4. The Owner runs the manual checks.

## Output / Outcome

| Output | Type | Description |
|---|---|---|
| Test files and QA assets | Repository files | Paths above |
| QA report | `.agent/reports/` | Measured values, verdict |
| Owner decisions | Plan file | Manual check results |

## Error handling

| Error condition | Handling strategy | Outcome |
|---|---|---|
| A test needs a binary not built | Helper panics with the correct command | Clear failure, not a silent skip |
| GPU busy with other programs | QA frees VRAM and records the baseline | Measurements comparable |
| An unexpected host in the proxy log | QA reports it to the Owner | Allow-list not widened silently |
| Watermark extractor API differs from the README call | QA reports it; architect revises the QA asset | FR-018 check blocked until fixed |

## Events emitted (if applicable)

Not applicable.

## Dependencies

| Dependency | Type | Required | Notes |
|---|---|---|---|
| `assert_cmd` 2.2.2, `predicates` 3.1.4, `tempfile` 3.27.0, `hound` 3.5.1, `serde_json` 1.0.154 | Build-time (dev) | yes | research P1 section 6 |
| `windows-sys` | Build-time (test kit) | yes | Same pinned version as `narrate` |
| `cargo-llvm-cov` | QA tool | yes | Coverage in step 4 |
| `nvidia-smi` (NVML) | QA tool | yes | Present with the NVIDIA driver |
| Inno Setup 6.7.3 | devops tool | yes | spec-narrate-installer.md |

## Sub-feature breakdown (if applicable)

Not applicable.

## Submodule scope (if applicable)

Not applicable.

## Traceability

| Requirement | Spec section | Verified by |
|---|---|---|
| FR-001 to FR-018, FR-024 | Coverage map | Listed test files, GPU QA items, installer checks |
| NFR-001 to NFR-009, NFR-011, NFR-012 | Coverage map; Requirement: GPU QA procedure | QA report (step 4), installer checks (step 5), Owner checks |
| NFR-004 | Requirement: QA assets (pitch script, estimator, voiced-frame rule); GPU QA item 5 | `qa/pitch/median_f0.py` output in the QA report |
| FR-011, NFR-001 (2026-10-13) | Change Log 2026-10-13: test kit proxy hosts; repository inspection; GPU QA item 14 | `pins.rs`, `setup_preflight.rs`, `setup_steps.rs`; GPU QA items 2.4 and 14 |
| FR-010, FR-024, FR-005 (2026-10-13) | Change Log 2026-10-13: automated setup-step cases (Kokoro on CPU, builtin load retry, sync phases); CLI usage cases; polled stdin reader | `setup_steps.rs`, `cli_errors.rs`; GPU QA item 3 |

## Change Log: 2026-10-13

**Source:** The 2026-10-13 change logs of spec-narrate-setup.md, spec-narrate-worker.md, spec-narrate-cli.md, and spec-narrate-architecture.md (user decisions "PyTorch wheel host" and "Build backend for source-only packages" in `DECISIONS.md`; step 3a findings against FR-005, FR-010, FR-011, FR-024). `docs/requirements.md` is unchanged.

### ADDED Requirements

#### Requirement: GPU QA item 14, locked build of `docopt`

QA SHALL add item 14 to the GPU QA procedure: during the scratch setup of item 2 (through the allow-list proxy), confirm that step `environment` installs `docopt` into `<scratch>\venv\Lib\site-packages`, that every directory named `setuptools-*.dist-info` under `<scratch>\uv-cache` is `setuptools-81.0.0.dist-info`, and that the proxy log for the whole setup has no `DENY` line and includes `download-r2.pytorch.org` only as `ALLOW` (FR-011, NFR-001).

##### Scenario: docopt built from the locked setuptools

- **GIVEN** the item 2 scratch setup has completed
- **WHEN** QA runs the item 14 checks
- **THEN** each check SHALL pass and the QA report SHALL list the `setuptools-*.dist-info` names found

### MODIFIED Requirements

#### Requirement: Test kit, `narrate-test-proxy` host list

**Was:** "tunnels `CONNECT host:port` when the host is in the allow-list of spec-narrate-setup.md (exact hosts plus suffixes `.hf.co`, `.huggingface.co`)"
**Now:** tunnels `CONNECT host:port` when the host is one of `pypi.org`, `files.pythonhosted.org`, `download.pytorch.org`, `download-r2.pytorch.org`, `github.com`, `objects.githubusercontent.com`, `release-assets.githubusercontent.com`, `huggingface.co`, or ends in `.hf.co` or `.huggingface.co` (spec-narrate-setup.md, Change Log 2026-10-13); every other part of the contract is unchanged.
**Reason:** User decision 2026-10-13, "PyTorch wheel host".

##### Scenario: R2 host allowed by the proxy

- **GIVEN** `narrate-test-proxy` running with a log file
- **WHEN** a client sends `CONNECT download-r2.pytorch.org:443`
- **THEN** the proxy SHALL tunnel it and log `ALLOW` for `download-r2.pytorch.org:443`

#### Requirement: Repository inspection

**Was:** "`python/uv.lock` lists `chatterbox-tts` 0.1.7, `torch` `2.6.0+cu124` and `torchaudio` `2.6.0+cu124` from `download.pytorch.org`, ... `python/pyproject.toml` declares the `pytorch-cu124` index with `explicit = true`"
**Now:** For `torch` and `torchaudio` (`2.6.0+cu124`), `pins.rs` SHALL assert `source = { registry = "https://download.pytorch.org/whl/cu124" }` and that every artifact URL starts with `https://download.pytorch.org/whl/` or `https://download-r2.pytorch.org/whl/`; no other package's source or artifact URL may use either prefix. `python/pyproject.toml` SHALL also contain, under `[tool.uv]`, `no-build-isolation-package = ["docopt"]` and `build-constraint-dependencies = ["setuptools==81.0.0"]`. `python/uv.lock` SHALL list `docopt` 0.6.2 with a hashed `sdist`. Manifest and `.gitattributes` checks read one line per dependency and per rule (spec-narrate-architecture.md, Change Log 2026-10-13). Every other assertion is unchanged.
**Reason:** User decisions 2026-10-13; the single-line form is what the test already reads.

##### Scenario: Pins recorded after the delta

- **GIVEN** the repository source after step 3a applies this delta
- **WHEN** `pins.rs` runs
- **THEN** every assertion above SHALL hold

#### Requirement: Automated setup-step tests (additional cases)

**Was:** the case list ended with "first run and re-run with the fakes; interrupted environment step".
**Now:** `setup_steps.rs` SHALL also cover the "(automated)" scenarios of the 2026-10-13 change logs: two sync phases in order (spec-narrate-setup.md); Kokoro clips rendered on the CPU; builtin load retried on the CPU after out of memory; CPU load also fails. The fake worker needs a knob that answers only the first `load` of a session with a given error code (suggested `FAKE_WORKER_FAIL_FIRST_LOAD=out_of_memory`). `cli_errors.rs` SHALL cover the three new usage-error scenarios of spec-narrate-cli.md (Change Log 2026-10-13), and `setup_preflight.rs` the R2 and project-metadata scenarios of spec-narrate-setup.md (Change Log 2026-10-13).
**Reason:** Step 3a findings and user decisions of 2026-10-13.

##### Scenario: New cases run without network

- **GIVEN** the fakes and fixtures of Requirement: Automated setup-step tests
- **WHEN** `cargo test --workspace --all-features` runs on a machine without network, uv, or Python
- **THEN** the new cases SHALL pass

### REMOVED Requirements

None.
