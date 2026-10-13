# spec-narrate-cli

## Metadata

| Field | Value |
|---|---|
| **Feature** | narrate command-line contract: grammar, output lines, exit codes, error catalog, environment variables, test hooks |
| **Parent feature** | spec-narrate-architecture.md |
| **Sub-features** | None |
| **Requirement refs** | UR-001, UR-002, UR-004, UR-006, UR-007, FR-001, FR-005, FR-006, FR-007, FR-008, FR-009, FR-013, FR-017, FR-018, FR-024, NFR-005, NFR-007, NFR-008 |
| **Feature type** | Other: command-line interface contract |
| **Status** | Review |

Status values: Draft, Review, Approved, Implemented.

Change Log 2026-10-13 (end of this file) tightens Requirement: Usage errors and adds a `source-not-allowed` cause form. Where the change log and the body differ, the change log wins.

## Description

This spec fixes every externally observable part of the `narrate` command line: the grammar, defaults, which stream each line goes to, the exact format of every line tests assert on, exit codes, the error catalog, and the environment variables (including the test hooks that exist only in `test-hooks` builds). Behavior behind the lines is specified in spec-narrate-generation.md (generation), spec-narrate-setup.md (`setup`, `status`), and spec-narrate-worker.md (worker). The developer-tester writes black-box tests from this file without reading the implementation.

## Triggered by

| Trigger type | Source | Condition |
|---|---|---|
| User action | Owner runs `narrate` in a terminal | Any invocation |
| Test harness | `assert_cmd` running `target\debug\narrate.exe` | Integration tests in `crates/narrate/tests/` |

## Required data

| Data | Source | Type | Notes |
|---|---|---|---|
| Arguments | Command line | Strings | Grammar below |
| `NARRATE_DATA_DIR` | Environment | Path | Optional; overrides `%LOCALAPPDATA%\narrate` (FR-017). Empty string counts as unset. The data directory `D` is `std::path::absolute(<value>)`, so relative values resolve against the current directory |
| Test-hook variables | Environment | Strings | Honored only when built with feature `test-hooks`; table below |
| All other environment variables | Environment | Strings | Passed to child processes except the prefixes removed in spec-narrate-setup.md (Child process environment) |

## Grammar

```text
narrate [GLOBAL] <FILE> [-o <PATH>] [--voice <VOICE> | --voice-file <WAV>] [--device <DEVICE>]
narrate [GLOBAL] setup
narrate [GLOBAL] status
narrate --help | -h
narrate --version | -V

GLOBAL: --verbose (accepted before or after the subcommand or FILE)
VOICE:  female | male | builtin       (default when neither --voice nor --voice-file is given: female)
DEVICE: auto | cuda | cpu             (default: auto)
```

| Element | Long | Short | Value | Default | Applies to |
|---|---|---|---|---|---|
| Script file | positional `<FILE>` | | Path with extension `.md`, `.markdown`, or `.txt` (case-insensitive) | Required for generation | Generation |
| Output | `--output` | `-o` | Path | `<FILE>` with its extension replaced by `.wav` | Generation |
| Voice preset | `--voice` | | `female`, `male`, `builtin` | `female` | Generation |
| Custom clip | `--voice-file` | | Path to a WAV file | None | Generation |
| Device | `--device` | | `auto`, `cuda`, `cpu` | `auto` | Generation |
| Verbose | `--verbose` | | Flag | Off | All commands |
| Help | `--help` | `-h` | Flag | | All |
| Version | `--version` | `-V` | Flag | | Top level |

Subcommand names win over the positional: `narrate setup` runs setup; a script named `setup.md` is a file because it is not exactly `setup`. `setup` and `status` take no options other than `--verbose` and `--help`. There is no option that disables or alters the watermark.

## Behavior

### Requirement: Usage errors

The system SHALL reject an unknown option, a missing `<FILE>`, an invalid `--voice` or `--device` value, and `--voice` combined with `--voice-file`, by printing an error and the usage to stderr and exiting 2, before touching the file system.

#### Scenario: Unknown option

- **GIVEN** any state
- **WHEN** the user runs `narrate --bogus`
- **THEN** the exit code SHALL be 2
- **AND** stderr SHALL contain `--bogus` and `Usage:`

#### Scenario: Invalid voice value

- **GIVEN** any state
- **WHEN** the user runs `narrate script.txt --voice robot`
- **THEN** the exit code SHALL be 2
- **AND** stderr SHALL contain `robot`, `female`, `male`, and `builtin`

#### Scenario: Invalid device value

- **GIVEN** any state
- **WHEN** the user runs `narrate script.txt --device gpu`
- **THEN** the exit code SHALL be 2
- **AND** stderr SHALL contain `auto`, `cuda`, and `cpu`

#### Scenario: Conflicting voice options

- **GIVEN** `my.wav` exists
- **WHEN** the user runs `narrate script.txt --voice male --voice-file my.wav`
- **THEN** the exit code SHALL be 2
- **AND** stderr SHALL contain `cannot be used with`
- **AND** no file SHALL be created in the script's directory

#### Scenario: No arguments

- **GIVEN** any state
- **WHEN** the user runs `narrate` with no arguments
- **THEN** the exit code SHALL be 2 and stderr SHALL contain `Usage:`

### Requirement: Help and version

The system SHALL print usage for `--help` and `-h`, and the version line for `--version` and `-V`, on stdout with exit code 0. The version line SHALL be exactly `narrate 0.1.0` in builds without `test-hooks` and exactly `narrate 0.1.0 (test hooks enabled)` in builds with it.

#### Scenario: Help lists commands and options

- **GIVEN** any state
- **WHEN** the user runs `narrate --help`
- **THEN** the exit code SHALL be 0
- **AND** stdout SHALL contain `setup`, `status`, `--output`, `--voice`, `--voice-file`, `--device`, `--verbose`, `--version`

#### Scenario: Version

- **GIVEN** any state
- **WHEN** the user runs `narrate --version`
- **THEN** the exit code SHALL be 0 and stdout SHALL start with `narrate 0.1.0`

#### Scenario: No watermark option

- **GIVEN** any state
- **WHEN** the user runs `narrate --help`, `narrate setup --help`, and `narrate status --help`
- **THEN** no output line SHALL match the regular expression `--\S*watermark`

### Requirement: Stream discipline

The system SHALL write only command results to stdout (the output WAV path after a successful generation, the `status` report, help, and version) and SHALL write every progress, notice, warning, verbose, and error line to stderr.

#### Scenario: Generation stdout

- **GIVEN** a successful generation of `C:\t\a.txt`
- **WHEN** the command exits 0
- **THEN** stdout SHALL be exactly `C:\t\a.wav` followed by one line break (`\r\n` or `\n`)

#### Scenario: Error goes to stderr

- **GIVEN** `nofile.txt` does not exist
- **WHEN** the user runs `narrate nofile.txt`
- **THEN** stdout SHALL be empty and stderr SHALL contain `error[input-not-found]`

### Requirement: Error message format

Every error that `narrate` itself reports SHALL be printed to stderr as a line `error[<code>]: <cause>` followed by a line `  fix: <next action>`, where `<code>` is from the error catalog; with `--verbose` the system SHALL add one or more `  detail: <text>` lines carrying the underlying error chain and, for worker errors, every line of the worker's traceback; without `--verbose` the system SHALL NOT print `detail:` lines or any line containing `Traceback (most recent call last)`.

#### Scenario: Cause and next action

- **GIVEN** `nofile.txt` does not exist
- **WHEN** the user runs `narrate nofile.txt`
- **THEN** stderr SHALL contain a line starting with `error[input-not-found]: ` that contains `nofile.txt`
- **AND** the next line SHALL start with `  fix: `

#### Scenario: Verbose error details

- **GIVEN** a ready data directory and a worker whose `synth` error reply carries a non-null `traceback`
- **WHEN** the user runs the generation with `--verbose`
- **THEN** stderr SHALL contain `  detail: ` lines that include the traceback text

#### Scenario: Details hidden without verbose

- **GIVEN** the same failing worker
- **WHEN** the user runs the generation without `--verbose`
- **THEN** stderr SHALL NOT contain `detail:` or `Traceback (most recent call last)`

### Requirement: Exit codes

The system SHALL exit with the code from this table, and with 0 only on success.

| Code | Meaning |
|---|---|
| 0 | Success (generation wrote the file; `setup` finished; `status` reports ready) |
| 1 | Generation runtime failure (worker, model, device, watermark, size limit, final write) |
| 2 | Usage error (argument parsing, invalid test-hook value) |
| 3 | Input, output, or voice-clip problem detected by `narrate` |
| 4 | Not set up: setup incomplete or pin set mismatch (generation and `status`) |
| 5 | Setup failure |
| 130 | Interrupted by Ctrl+C or Ctrl+Break |

#### Scenario: Not-ready status code

- **GIVEN** an empty scratch data directory
- **WHEN** the user runs `narrate status`
- **THEN** the exit code SHALL be 4

#### Scenario: Input error code

- **GIVEN** `story.docx` exists
- **WHEN** the user runs `narrate story.docx`
- **THEN** the exit code SHALL be 3

### Requirement: Test hooks honored only in test builds

A binary built with feature `test-hooks` SHALL honor the test-hook variables below and SHALL pass `NARRATE_TEST_FAIL_CHUNK`, `NARRATE_TEST_CRASH_CHUNK`, and `NARRATE_TEST_WATERMARK_FAIL` to the worker; a binary built without it SHALL ignore all of them and SHALL remove every variable starting with `NARRATE_TEST_` from child environments.

| Variable | Value | Effect | Implemented by |
|---|---|---|---|
| `NARRATE_WORKER_CMD` | Absolute path of an executable | Run this executable, with no arguments, instead of the Python worker, for every worker session (generation, `status` probe, setup) | `narrate` |
| `NARRATE_UV` | Absolute path of an executable | Use it as uv instead of searching PATH for `uv.exe` | `narrate` |
| `NARRATE_TEST_FAIL_CHUNK` | Integer N >= 1 | The worker answers `synth` for chunk N with error code `test_failure` | Worker (real and fake) |
| `NARRATE_TEST_CRASH_CHUNK` | Integer N >= 1 | The worker exits with code 3 without replying when it receives `synth` for chunk N | Worker (real and fake) |
| `NARRATE_TEST_WATERMARK_FAIL` | `1` | The worker answers `load` with error code `watermarker_unavailable` | Worker (real and fake) |
| `NARRATE_TEST_WAV_LIMIT_BYTES` | Integer >= 44 | Replaces the WAV size limit 4294967295 (NFR-005) | `narrate` |
| `NARRATE_TEST_PINS_FILE` | Path | Replaces the embedded `pins.json` for this run (changes the pin set id) | `narrate` |
| `NARRATE_TEST_LOCK_FILE` | Path | Replaces the embedded `uv.lock` for this run (changes the pin set id) | `narrate` |

#### Scenario: Hook active in test build

- **GIVEN** a `test-hooks` build, a ready data directory, the fake worker, and a 5-chunk script
- **WHEN** the user runs generation with `NARRATE_TEST_FAIL_CHUNK=3`
- **THEN** the exit code SHALL be 1 and stderr SHALL contain `error[chunk-failed]` and `chunk 3 of 5`

#### Scenario: Invalid hook value

- **GIVEN** a `test-hooks` build
- **WHEN** the user runs any generation with `NARRATE_TEST_FAIL_CHUNK=abc`
- **THEN** the exit code SHALL be 2 and stderr SHALL contain `error[invalid-test-hook]` and `NARRATE_TEST_FAIL_CHUNK`

#### Scenario: Hook ignored in release build

- **GIVEN** a release build without `test-hooks`
- **WHEN** the user runs `narrate status` with `NARRATE_WORKER_CMD` set to a nonexistent path
- **THEN** the command SHALL behave as if the variable were unset

## Line catalog

Path rule (applies to every path `narrate` prints or sends to a worker: the stdout output path, `voice:`, `notice:`, `done:`, `verbose: data directory`, the status `data directory:` line, and the `clip` field of `voice` requests): a user-given path (`<FILE>`, `-o`, `--voice-file`) is printed and sent as `std::path::absolute(<value as typed>)`; the default output path is that absolute input path with its extension replaced by `wav`; data-directory paths are `D` (spec-narrate-architecture.md, Data directory resolution) joined with the fixed components named in the specs (for example `D\voices\female.wav`). `std::fs::canonicalize` is used only for the output-is-input comparison (spec-narrate-generation.md, precondition 5) and its result is never printed or sent, so no printed path carries the `\\?\` prefix and letter case is kept as typed.

Placeholders: `<path>` follows the path rule above; `<i>` and `<N>` are 1-based chunk number and chunk count; seconds use one decimal (`{:.1}`) and RTF two decimals (`{:.2}`). All lines go to stderr unless marked stdout.

| Prefix | Exact format | Printed when |
|---|---|---|
| `voice:` | `voice: female (<path>)`, `voice: male (<path>)`, `voice: builtin`, `voice: file (<path>)` | Once, after all checks pass and before the worker starts (FR-007, FR-008, FR-009) |
| `notice:` | `notice: <path> is <s> s long; only the first 10 s are used` | After the `voice:` line, only when the custom clip is longer than 10 s (FR-009) |
| `warning:` | `warning: unknown tag [<tag>] will be read as text` | Once per distinct unknown tag, after the `voice:` line (FR-003) |
| `warning:` | `warning: CUDA is not available; falling back to CPU (generation will be slower)` | `--device auto` resolved to CPU (FR-013) |
| `warning:` | `warning: chunk <i>/<N> reached the 40 s model limit; its speech may be cut off` | A chunk returned 948000 or more samples |
| `device:` | `device: cuda (<device name>)` or `device: cpu` | After the model loads (FR-013) |
| `chunk` | `chunk <i>/<N> done` | After each chunk's samples are appended |
| `done:` | `done: <path> (<a> s audio, <n> samples, synthesis <s> s, rtf <r>)` | After the output file is renamed into place |
| stdout | `<path>` | After the `done:` line |
| `verbose:` | `verbose: data directory <path>`, `verbose: chunks <N>`, `verbose: worker pid <pid>`, `verbose: model load <s> s`, `verbose: chunk <i>/<N> text: <text>`, `verbose: chunk <i>/<N> samples=<n>`, `verbose: silence after chunk <i> samples=<n>` | Only with `--verbose` (FR-005, FR-024) |
| `worker:` | `worker: <line>` | Only with `--verbose`: each line the worker writes to stderr |
| `error[` | `error[<code>]: <cause>` | On failure |
| `  fix:` | `  fix: <next action>` | Immediately after each error line |
| `  detail:` | `  detail: <text>` | Only with `--verbose`, after the fix line |

The verbose sample lines match `^verbose: chunk (\d+)/(\d+) samples=(\d+)$` and `^verbose: silence after chunk (\d+) samples=(\d+)$`. The output WAV's sample count equals the sum of every `samples=` value on those lines (FR-005). Setup and status lines are in spec-narrate-setup.md.

## Error catalog

`<p>` is the path exactly as the user typed it (or, for the default output, the typed input path with its extension replaced by `wav`); `<dir>` is the parent directory of `<p>` or the data directory `D`; `<os>` is the operating-system error text.

| Code | Exit | Cause line after `error[<code>]: ` | Fix line after `  fix: ` | Raised when |
|---|---|---|---|---|
| `input-extension` | 3 | `unsupported script type: <p>` | `use a .md, .markdown, or .txt file` | Extension not allowed (checked before existence) |
| `input-not-found` | 3 | `input file not found: <p>` | `check the path and try again` | File does not exist |
| `input-not-a-file` | 3 | `input path is not a file: <p>` | `pass a script file, not a directory` | Path is a directory |
| `input-unreadable` | 3 | `cannot read <p>: <os>` | `check the file permissions` | Read fails |
| `input-encoding` | 3 | `<p> is not valid UTF-8 (invalid byte at offset <n>)` | `save the file with UTF-8 encoding` | Decoding fails (FR-001) |
| `script-empty` | 3 | `<p> has no speakable text after conversion` | `add text outside code blocks and front matter` | No segment after conversion (FR-004) |
| `output-dir-missing` | 3 | `output directory does not exist: <dir>` | `create the directory or choose another -o path` | Parent of the output path missing (FR-006) |
| `output-is-directory` | 3 | `output path is a directory: <p>` | `give a file name, for example -o <p>\out.wav` | Output path is a directory |
| `output-is-input` | 3 | `the output would overwrite the input file: <p>` | `choose a different -o path` | Output resolves to the input (FR-006) |
| `output-not-writable` | 3 | `output path is not writable: <p> (<os>)` | `choose another directory, or close programs that use the file` | Probe or final rename fails with a permission or sharing error (FR-006) |
| `voice-file-not-found` | 3 | `voice file not found: <p>` | `check the path to the WAV clip` | `--voice-file` path missing (FR-009) |
| `voice-file-unreadable` | 3 | `<p> is not a readable WAV file (<detail>)` | `export the clip as a WAV file and try again` | Not RIFF/WAVE, malformed, or truncated (FR-009) |
| `voice-file-unsupported` | 3 | `<p> uses an unsupported WAV encoding (<detail>); supported encodings: PCM 8, 16, 24, or 32-bit integer, or 32-bit float` | `convert the clip to 16-bit PCM WAV` | Readable header, unsupported format (FR-009) |
| `voice-file-too-short` | 3 | `<p> is <s> s long; the voice clip must be longer than 5 s` | `record or export a clip longer than 5 s (10 s or more is best)` | Duration 5.0 s or less (FR-009) |
| `not-set-up` | 4 | `setup is incomplete: <component list>` | `run narrate setup` | Any readiness component missing (FR-015, FR-008, FR-012) |
| `pin-mismatch` | 4 | `the data directory <dir> was set up with a different pin set (recorded <a>, this binary <b>)` | `run narrate setup` | `setup.json` pin set id differs (FR-017) |
| `worker-start` | 1 | `could not start the generation worker: <os>` | `run narrate setup to repair the environment` | Spawn fails or no `hello` reply in 60 s |
| `worker-protocol` | 1 | `the generation worker sent an invalid message: <detail>` | `run narrate setup to repair the environment` | Non-JSON line, wrong id, missing field, wrong protocol or sample rate |
| `worker-exited` | 1 | `the generation worker exited unexpectedly (exit code <c>) during chunk <i> of <N>` | `rerun with --verbose, or read <log path>` | Worker stdout closed before a reply. Outside `synth`, `during chunk <i> of <N>` becomes `while <op>` |
| `cuda-unavailable` | 1 | `CUDA is not available on this machine` | `use --device auto or --device cpu, or check the NVIDIA driver` | `--device cuda` without CUDA (FR-013) |
| `model-load` | 1 | `could not load the Chatterbox-Turbo model: <message>` | `run narrate setup to repair the model files` | Worker `model_load_failed` |
| `watermark-unavailable` | 1 | `the PerTh watermarker could not be loaded; narrate does not write unwatermarked audio` | `run narrate setup to repair the environment` | Worker `watermarker_unavailable` (FR-018) |
| `voice-prepare` | 1 | `could not prepare the <voice> voice: <message>` | `use another clip or run narrate setup` | Worker `voice_failed` or `builtin_unavailable` |
| `chunk-failed` | 1 | `generation failed on chunk <i> of <N>: <message>` | `rerun with --verbose for details` | Worker `synth_failed` or `test_failure` (FR-005) |
| `gpu-out-of-memory` | 1 | `the GPU ran out of memory on chunk <i> of <N>` | `close other programs that use the GPU, or use --device cpu` | Worker `out_of_memory` |
| `wav-too-large` | 1 | `the audio would exceed the WAV size limit of <limit> bytes` | `split the script into smaller files` | Size check (NFR-005) |
| `output-write` | 1 | `could not write <p>: <os>` | `check free disk space and try again` | Writing the temporary WAV fails for a reason other than permission |
| `interrupted` | 130 | `interrupted; no output was written` | `run the command again` | Ctrl+C or Ctrl+Break during generation |
| `uv-not-found` | 5 | `uv was not found on PATH` (or `uv was not found at <path>` when `NARRATE_UV` names a missing file) | Two fix lines, in this order: `download https://github.com/astral-sh/uv/releases/download/0.10.2/uv-x86_64-pc-windows-msvc.zip, check that its sha256 is 493ebbe0e06128d6ee4905e1ed5e2a433fb0f7cfc08b0eaca9fab4ca76778ae1, and put uv.exe on PATH`, then `or run: powershell -ExecutionPolicy ByPass -c "irm https://astral.sh/uv/0.10.2/install.ps1 \| iex"` | Setup preflight (FR-010) |
| `uv-version` | 5 | `uv <found> is installed, but narrate needs uv 0.10.2` | Same two fix lines as `uv-not-found` | Setup preflight (FR-011) |
| `source-not-allowed` | 5 | One of: `<url> is not an allowed download source`; `<package> must come from https://download.pytorch.org/whl/ (the lock uses <url>)`; `<package> may not come from https://download.pytorch.org/whl/`; `Hugging Face repository <repo> is not allowed`; `<package> uses a <kind> source, which is not allowed` (`<kind>` is `git`, `path`, `directory`, or `editable`) | `restore python/uv.lock and pins.json from the narrate release` | Allow-list check (FR-011), spec-narrate-setup.md Requirement 1 |
| `pin-invalid` | 5 | One of: `uv.lock line <n> has an unrecognized form`; `uv.lock line <n> (package <package>) has an artifact without a sha256 hash`; `uv.lock package <package> is missing its <field>` (`<field>` is `name`, `version`, `source`, or `hashed artifact`; `<package>` is `at line <n>` when the name is missing); `pins.json is invalid: <serde_json error text>`; `pins.json <field path> must be <expected>, found <value>`; `pins.json path <value> is not a safe relative path` | `restore python/uv.lock and pins.json from the narrate release` | Lock scanner and pin rules, spec-narrate-setup.md Requirement 1 |
| `hash-mismatch` | 5 | `<artifact> does not match its pinned sha256 (expected <e>, got <a>); the file was discarded` | `run narrate setup again; if this repeats, do not use this network` | Model file or Python archive verification (NFR-001) |
| `setup-step-failed` | 5 | `setup failed at step <step>: <message>` | `check the network connection and run narrate setup again; completed steps are kept` | Any setup step failure not covered above (FR-010) |
| `setup-busy` | 5 | `another narrate setup is running for <dir>` | `wait for it to finish, then run narrate setup again` | Lock held |
| `data-dir-unwritable` | 5 | `cannot write to the data directory <dir>: <os>` | `set NARRATE_DATA_DIR to a writable directory` | Creating the data directory or a file in it fails |
| `setup-interrupted` | 130 | `setup was interrupted` | `run narrate setup again to continue` | Ctrl+C or Ctrl+Break during setup |
| `invalid-test-hook` | 2 | `<VARIABLE> has an invalid value: <value>` | `unset it or give a valid value` | Test-hook value fails to parse (test-hooks builds only) |
| `internal` | 1 | `<message>` | `rerun with --verbose and report the output` | Any failure not covered above (a defect) |

Note on `uv-not-found` and `uv-version`: the hash-verifiable zip comes first because its URL and sha256 are research-verified (research P1 section 3.3, GitHub release asset digest). The PowerShell installer line is second because it downloads and runs a script without a pinned hash; it comes from Astral's documentation, not the research notes, so QA confirms the command works. The `\|` in the table is Markdown escaping; the printed line contains a plain `|`, exactly `  fix: or run: powershell -ExecutionPolicy ByPass -c "irm https://astral.sh/uv/0.10.2/install.ps1 | iex"`.

## Algorithmic steps

1. Parse arguments; usage errors exit 2.
2. Read test-hook variables (test-hooks builds); invalid values exit 2.
3. Dispatch: `setup` (spec-narrate-setup.md), `status` (spec-narrate-setup.md), or generation (spec-narrate-generation.md).
4. Map any failure to one catalog entry; print the error, fix, and (with `--verbose`) detail lines; exit with the entry's code.

## Output / Outcome

| Output | Type | Description |
|---|---|---|
| Output WAV | File | spec-narrate-generation.md |
| stdout | Text | Output path, status report, help, version |
| stderr | Text | Line catalog above |
| Exit code | Integer | Exit code table above |

## Error handling

| Error condition | Handling strategy | Outcome |
|---|---|---|
| Argument parsing fails | clap error with usage | Exit 2, nothing touched |
| Any catalog condition | Single error line plus fix line | Exit code from the catalog |
| Unexpected internal error (bug) | Catch at top level, report as `error[internal]: <message>` with fix `rerun with --verbose and report the output` | Exit 1; with `--verbose` the full error chain as detail lines |

## Events emitted (if applicable)

Not applicable: the CLI emits no events; worker messages are in spec-narrate-worker.md.

## Dependencies

| Dependency | Type | Required | Notes |
|---|---|---|---|
| `clap` 4.6.7 (derive) | Build-time | yes | Parsing, help, usage errors (exit 2) |
| `anyhow` 1.0.104, `thiserror` 2.0.21 | Build-time | yes | Error chain for `detail:` lines |
| spec-narrate-generation.md, spec-narrate-setup.md | Spec | yes | Behavior behind the lines |

## Traceability

| Requirement | Spec section | Verified by |
|---|---|---|
| UR-006 | Requirement: Error message format; Error catalog | CLI tests `crates/narrate/tests/cli_errors.rs` |
| UR-007 | Grammar (`-o`) | CLI tests `crates/narrate/tests/output.rs` |
| FR-001 | Error catalog (`input-*`) | CLI tests `crates/narrate/tests/input.rs` |
| FR-005 | Requirement: Test hooks honored only in test builds; Line catalog (verbose sample lines) | CLI tests `crates/narrate/tests/chunks.rs` |
| FR-006 | Error catalog (`output-*`) | CLI tests `crates/narrate/tests/output.rs` |
| FR-007, FR-008, FR-009 | Grammar; Line catalog (`voice:`, `notice:`) | CLI tests `crates/narrate/tests/voices.rs` |
| FR-013 | Grammar (`--device`); Line catalog (`device:`, fallback warning) | CLI tests `crates/narrate/tests/device.rs` |
| FR-017 | Required data (`NARRATE_DATA_DIR`) | CLI tests `crates/narrate/tests/status.rs` |
| FR-018 | Scenario: No watermark option | CLI tests `crates/narrate/tests/cli_errors.rs` |
| FR-024 | Requirements: Usage errors; Help and version; Error message format | CLI tests `crates/narrate/tests/cli_errors.rs` |
| FR-005 (release builds ignore hooks) | Scenario: Hook ignored in release build | GPU QA item 13 (the automated suite always builds with `test-hooks`) |
| FR-024 (2026-10-13) | Change Log 2026-10-13: Requirement: Usage errors (global `--verbose`, generation options with a subcommand, usage line after a suggestion) | `crates/narrate/tests/cli_errors.rs` |
| FR-011 (2026-10-13) | Change Log 2026-10-13: `source-not-allowed` row | `crates/narrate/tests/setup_preflight.rs` |

## Change Log: 2026-10-13

**Source:** Implementation finding from plan step 3a against FR-024 (clap's `args_conflicts_with_subcommands` made `narrate --verbose status` read `status` as `<FILE>`, so the implementation parses subcommands and generation options together and rejects the combination itself); user decision `DECISIONS.md` 2026-10-13 "PyTorch wheel host" (FR-011). `docs/requirements.md` is unchanged.

### ADDED Requirements

None.

### MODIFIED Requirements

#### Requirement: Usage errors

**Was:** "The system SHALL reject an unknown option, a missing `<FILE>`, an invalid `--voice` or `--device` value, and `--voice` combined with `--voice-file`, by printing an error and the usage to stderr and exiting 2, before touching the file system."
**Now:** The system SHALL reject an unknown option, a missing `<FILE>`, an invalid `--voice` or `--device` value, `--voice` combined with `--voice-file`, and any generation element (`<FILE>`, `-o`/`--output`, `--voice`, `--voice-file`, `--device`) given together with `setup` or `status`, by printing an error and a line starting with `Usage:` to stderr and exiting 2, before touching the file system. Three rules make this exact:

1. `--verbose` is global: `narrate --verbose status`, `narrate status --verbose`, `narrate --verbose setup`, and `--verbose` before or after `<FILE>` are all valid. `narrate --verbose status` runs `status` and never treats `status` as `<FILE>`.
2. A generation element after the subcommand (for example `narrate status --device cpu`) is an unexpected argument for that subcommand (clap's message). A generation element before or around it (for example `narrate --device cpu status` or `narrate script.txt status`) gives the line `error: generation options cannot be used with the setup and status commands`. Both exit 2, print `Usage:`, and run neither the subcommand nor a generation.
3. Every exit-2 usage error prints a `Usage:` line, including clap errors that show a suggestion (`tip:`) and would otherwise omit the usage, for example `--voice Female`.

**Reason:** Step 3a showed the original wording left these three cases open, and clap's default for case 3 omits the usage that FR-024 requires.

##### Scenario: Global verbose before a subcommand

- **GIVEN** `NARRATE_DATA_DIR` names an empty scratch directory
- **WHEN** the user runs `narrate --verbose status`
- **THEN** the exit code SHALL be 4 and stdout SHALL end with `status: not ready (run narrate setup)`
- **AND** stderr SHALL NOT contain `error[input-extension]` or `Usage:`

##### Scenario: Generation option with a subcommand

- **GIVEN** `NARRATE_DATA_DIR` names an empty scratch directory
- **WHEN** the user runs `narrate --device cpu status`, and separately `narrate status --device cpu`
- **THEN** each SHALL exit 2 with `Usage:` on stderr and empty stdout
- **AND** the first SHALL print `error: generation options cannot be used with the setup and status commands`
- **AND** the scratch directory SHALL still be empty

##### Scenario: Usage line after a value suggestion

- **GIVEN** any state
- **WHEN** the user runs `narrate script.txt --voice Female`
- **THEN** the exit code SHALL be 2 and stderr SHALL contain `Female`, `female`, and a line starting with `Usage:`

#### Requirement: Error catalog, `source-not-allowed` row

**Was:** cause forms included "`<package> may not come from https://download.pytorch.org/whl/`".
**Now:** that form becomes `<package> may not come from <prefix>`, where `<prefix>` is `https://download.pytorch.org/whl/` or `https://download-r2.pytorch.org/whl/` (whichever matched). `<package> must come from https://download.pytorch.org/whl/ (the lock uses <url>)` now also covers a `torch` or `torchaudio` artifact URL outside both PyTorch prefixes. The other cause forms, exit code, and fix line are unchanged.
**Reason:** User decision 2026-10-13, "PyTorch wheel host" (spec-narrate-setup.md, Change Log 2026-10-13).

##### Scenario: R2 cause form

- **GIVEN** a test build, the fake uv printing `uv 0.10.2 (test)`, and a lock copy that moves a `numpy` wheel URL to `https://download-r2.pytorch.org/whl/cu124/`
- **WHEN** the user runs `narrate setup`
- **THEN** stderr SHALL contain `error[source-not-allowed]: numpy may not come from https://download-r2.pytorch.org/whl/`

### REMOVED Requirements

None.
| NFR-005 | Test hook `NARRATE_TEST_WAV_LIMIT_BYTES`; `wav-too-large` | CLI tests `crates/narrate/tests/output.rs` |
| NFR-007 | Requirement: Error message format | CLI tests (every error test asserts the `  fix:` line) |
| NFR-008 | Requirement: Exit codes | CLI tests (every test asserts the exit code) |
