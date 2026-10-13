# Requirements: narrate (voice-audio-creator-alpha)

Author: Toni-Jan Keith Monserrat
Status: Draft for architect (spec comes next)
Keywords SHALL, MUST, SHOULD, MAY follow RFC 2119.

## 1. Scope

`narrate` is a Windows command-line tool that converts a script file (Markdown or plain text) into a spoken WAV file using Chatterbox-Turbo, entirely on the user's machine. It uses the GPU when available.

- Phase 1: Rust CLI `narrate` with a Python/uv-managed Chatterbox-Turbo runtime, plus an Inno Setup installer.
- Phase 2 (starts only after Phase 1 works): separate experimental binary `narrate-onnx` (pure Rust + ONNX Runtime) and a backend comparison report. `narrate` stays unchanged.

Target machine: Windows 11, NVIDIA RTX 3050 8 GB, Rust 1.99, uv 0.10.2.

### Out of scope

| Item | Reason | Deferred to |
|---|---|---|
| Subjective voice quality ("beautiful") automated scoring | No automated check can judge it; user judges by listening | Never |
| Disabling the PerTh watermark in `narrate` | Decision: watermark stays on | Never |
| Non-Windows platforms, admin/system-wide install | Not requested | Never |
| Voices other than female, male, builtin, custom clip | Not requested | Never |
| Output formats other than WAV | Not requested | Never |
| Changing `narrate` to use ONNX | `narrate-onnx` is a separate binary | Never |

## 2. Business requirements

| ID | Statement | Phase |
|---|---|---|
| BR-001 | To turn written scripts into narration without cloud services, the business must provide a tool that generates voice audio locally from a script file. | 1 |
| BR-002 | To make the tool usable immediately, the business must deliver a Windows installer that needs no admin rights, with the tool installed on the owner's machine. | 1 |
| BR-003 | To minimize security and supply-chain risk, the business must obtain all software and model files only from official sources, pinned to exact versions and verified by hash. | 1 |
| BR-004 | To decide which runtime to keep, the business must compare the Python-based backend with a pure-Rust ONNX backend on the same machine. | 2 |

## 3. User requirements

Role: Owner (the single end user).

| ID | Statement | Traces to |
|---|---|---|
| UR-001 | The Owner shall be able to run one command with a `.md`, `.markdown`, or `.txt` script and obtain a spoken WAV file. | BR-001 |
| UR-002 | The Owner shall be able to choose a female (default), male, built-in, or custom-clip voice. | BR-001 |
| UR-003 | The Owner shall be able to run a one-time setup command after which narration works with no network. | BR-001 |
| UR-004 | The Owner shall be able to use the GPU automatically and override the device. | BR-001 |
| UR-005 | The Owner shall be able to install `narrate` with a double-click installer and call it from any terminal. | BR-002 |
| UR-006 | The Owner shall be able to understand and fix any failure from the error message, and check setup status. | BR-001, BR-002 |
| UR-007 | The Owner shall be able to choose the output path. | BR-001 |
| UR-008 | The Owner shall be able to run the experimental `narrate-onnx` on the same scripts. | BR-004 |
| UR-009 | The Owner shall be able to read a report comparing both backends. | BR-004 |
| UR-010 | The Owner shall be able to trust that every downloaded software and model file comes from an official source, is pinned, and is verified by hash, and that installing and running do not change anything outside the tool's own directories, except the user PATH entry, the installer's HKCU uninstall registration, and the output WAV file and its temporary file in the output directory. | BR-003 |

## 4. Functional requirements

Each FR lists its parent UR and phase. Acceptance scenarios that delete, corrupt, or start from an empty data directory SHALL run with `NARRATE_DATA_DIR` pointing at a scratch directory, so they never touch the Owner's real setup (see FR-017). Happy-path scenarios MAY use a complete data directory, including the default, provided they do not modify it.

### Phase 1

### FR-001 Input file types and encoding (UR-001, Phase 1)
The system SHALL accept an input file with extension `.md`, `.markdown`, or `.txt` (case-insensitive) and SHALL reject any other extension. The system SHALL read the file as UTF-8, with or without a byte-order mark, and SHALL reject invalid UTF-8 with an error naming the file.

#### Scenario: Supported file
- **GIVEN** setup is complete and `story.txt` exists
- **WHEN** the user runs `narrate story.txt`
- **THEN** the system SHALL generate audio from the file

#### Scenario: BOM accepted
- **GIVEN** `story.txt` is UTF-8 with a byte-order mark
- **WHEN** the user generates audio
- **THEN** the text sent to the model SHALL NOT contain the byte-order mark character

#### Scenario: Invalid UTF-8
- **GIVEN** `bad.txt` contains bytes that are not valid UTF-8
- **WHEN** the user runs `narrate bad.txt`
- **THEN** the system SHALL exit non-zero and name `bad.txt` and the encoding problem in the message

#### Scenario: Unsupported extension
- **GIVEN** `story.docx` exists
- **WHEN** the user runs `narrate story.docx`
- **THEN** the system SHALL exit non-zero and name the supported extensions in the message
- **AND** SHALL NOT write an output file

#### Scenario: Missing file
- **GIVEN** `nofile.txt` does not exist
- **WHEN** the user runs `narrate nofile.txt`
- **THEN** the system SHALL exit non-zero and state the path that was not found

### FR-002 Markdown to spoken text (UR-001, Phase 1)
For Markdown input, the system SHALL convert the document to plain spoken text: strip syntax, read link text (not the URL), skip fenced and indented code blocks, and insert a pause after each heading and each paragraph. The system SHALL also apply these rules:
- Inline code: read the code text without backticks.
- Images: read the alt text; skip the image when alt text is empty.
- Lists: read each item as its own sentence with the list marker removed.
- Tables: read cell text row by row, with a pause between cells.
- Block quotes: read the text with the quote marker removed.
- Raw HTML: remove tags and keep the text content.
- YAML front matter at the top of the file: skip.

#### Scenario: Markdown converted
- **GIVEN** a `.md` file containing a heading, `**bold**`, a link `[docs](https://x.y)`, and a fenced code block
- **WHEN** the user generates audio
- **THEN** the text sent to the model SHALL contain the heading text, "bold", and "docs"
- **AND** SHALL NOT contain `**`, `https://x.y`, or the code block content
- **AND** the audio SHALL include a pause after the heading and after each paragraph

#### Scenario: Other elements
- **GIVEN** a `.md` file with YAML front matter, an image `![a cat](c.png)`, a list item `- one`, and `` `code` ``
- **WHEN** converted to spoken text
- **THEN** the text SHALL NOT contain the front matter, `c.png`, `- `, or backticks
- **AND** SHALL contain "a cat", "one", and "code"

#### Scenario: Code-only Markdown
- **GIVEN** a `.md` file containing only a code block
- **WHEN** the user generates audio
- **THEN** the system SHALL fail with the empty-script error from FR-004

### FR-003 Plain text and paralinguistic tags (UR-001, Phase 1)
For `.txt` input, the system SHALL use the content as-is. For all inputs, the system SHALL pass Turbo tags such as `[laugh]`, `[chuckle]`, and `[cough]` through to the model unchanged.

#### Scenario: Tag preserved
- **GIVEN** a script containing `Well [chuckle] that worked.`
- **WHEN** the user generates audio
- **THEN** the text sent to the model SHALL contain `[chuckle]` unchanged

#### Scenario: Tag in Markdown
- **GIVEN** a `.md` script containing `[laugh]` not followed by a link target
- **WHEN** converted to spoken text
- **THEN** the system SHALL keep `[laugh]` and SHALL NOT treat it as link text

#### Scenario: Unknown or malformed tag
- **GIVEN** a script containing `[dance]` and an unclosed `[laugh`
- **WHEN** the user generates audio
- **THEN** the system SHALL pass both through unchanged and SHALL NOT fail because of them

### FR-004 Empty script (UR-006, Phase 1)
The system SHALL fail with a clear message when the script has no speakable text after conversion.

#### Scenario: Empty file
- **GIVEN** an empty `.txt` file
- **WHEN** the user runs `narrate empty.txt`
- **THEN** the system SHALL exit non-zero with a message that the script has no speakable text
- **AND** SHALL NOT write an output file

#### Scenario: Whitespace only
- **GIVEN** a `.txt` file containing only spaces and newlines
- **WHEN** the user runs `narrate` on it
- **THEN** the system SHALL fail with the same message

### FR-005 Chunking, joining, and interruption (UR-001, Phase 1)
The system SHALL split long scripts at sentence boundaries into chunks no larger than the model limit, generate each chunk, and join the results with a short silence into one audio file. When the user presses Ctrl+C or the parent process is killed, the generation worker SHALL exit within 10 seconds (estimate; the spec fixes the bound) and SHALL release GPU memory, and no output file or temporary file SHALL remain from that run. With `--verbose`, the system SHALL print each chunk's audio duration and each inserted silence as integer sample counts. The system SHALL provide test hooks (environment variables named in the spec) that make the worker fail on chunk N, make the watermarker fail to load, and lower the WAV size limit to simulate an overflow (see NFR-005), and a way to run the chunker without the model.

#### Scenario: Chunker completeness and order
- **GIVEN** a script longer than one model chunk
- **WHEN** the chunker runs without the model
- **THEN** the chunks concatenated in order SHALL equal the spoken text after whitespace normalization
- **AND** no sentence SHALL be cut mid-sentence except when a single sentence exceeds the chunk limit

#### Scenario: Long script output
- **GIVEN** a script longer than one model chunk
- **WHEN** the user generates audio
- **THEN** the system SHALL produce one WAV file
- **AND** with `--verbose`, the system SHALL print each chunk and silence duration as an integer sample count
- **AND** the WAV's total sample count SHALL equal the sum of the printed counts exactly

#### Scenario: One chunk fails
- **GIVEN** the test hook makes the worker fail on chunk 3 of 5
- **WHEN** generation runs
- **THEN** the system SHALL exit non-zero, report the chunk number, and SHALL NOT leave a partial output file

#### Scenario: Interrupted run
- **GIVEN** generation is running on CUDA
- **WHEN** the user presses Ctrl+C, or the parent process is killed
- **THEN** the worker process SHALL no longer exist within 10 seconds and its GPU memory SHALL be released (checked through NVML)
- **AND** no output or temporary file SHALL remain

### FR-006 Output file (UR-001, UR-007, Phase 1)
The system SHALL write a WAV file at the model's native sample rate. The default path SHALL be the input path with extension `.wav`. The `-o` / `--output` option SHALL override it. The system SHALL overwrite an existing output file. The system SHALL write to a temporary file in the output directory and rename it on success, so a failed run keeps any earlier output file unchanged. The system SHALL reject an output path that resolves to the input file.

#### Scenario: Output equals input
- **GIVEN** `intro.txt` exists
- **WHEN** the user runs `narrate intro.txt -o intro.txt` (or any path that resolves to the same file)
- **THEN** the system SHALL exit non-zero stating the output would overwrite the input
- **AND** `intro.txt` SHALL be unchanged

#### Scenario: Default output
- **GIVEN** `C:\scripts\intro.md`
- **WHEN** the user runs `narrate C:\scripts\intro.md`
- **THEN** the system SHALL write `C:\scripts\intro.wav`

#### Scenario: Override
- **GIVEN** `-o D:\out\a.wav` where `D:\out` exists
- **WHEN** the user runs the command
- **THEN** the system SHALL write the file at `D:\out\a.wav`

#### Scenario: Output directory missing
- **GIVEN** `-o D:\nodir\a.wav` where `D:\nodir` does not exist
- **WHEN** the user runs the command
- **THEN** the system SHALL exit non-zero and state that the output directory does not exist

#### Scenario: Permission denied
- **GIVEN** the output directory has a deny-write ACL for the current user (set with `icacls`, no admin rights)
- **WHEN** the user runs the command
- **THEN** the system SHALL exit non-zero and state that the path is not writable

#### Scenario: Existing output survives failure
- **GIVEN** `intro.wav` exists and the test hook makes generation fail
- **WHEN** the user runs `narrate intro.md`
- **THEN** `intro.wav` SHALL be byte-identical to before the run

#### Scenario: Existing output overwritten
- **GIVEN** `intro.wav` exists and generation succeeds
- **WHEN** the user runs `narrate intro.md`
- **THEN** `intro.wav` SHALL contain the new audio

### FR-007 Voice selection female and male (UR-002, Phase 1)
The system SHALL provide `--voice female` (default when `--voice` is omitted) and `--voice male`. Female SHALL clone a reference clip generated at setup with Kokoro-82M voice `af_heart`. Male SHALL clone a clip generated with Kokoro voice `am_michael`. Before generation starts, the system SHALL print to standard error a line starting with `voice:` that names the preset and the reference clip path (or `builtin`).

#### Scenario: Default is female
- **GIVEN** setup is complete
- **WHEN** the user runs `narrate script.txt` without `--voice`
- **THEN** the `voice:` line SHALL name `female` and the path of the `af_heart` clip

#### Scenario: Male voice
- **GIVEN** setup is complete
- **WHEN** the user runs `narrate script.txt --voice male`
- **THEN** the `voice:` line SHALL name `male` and the path of the `am_michael` clip

#### Scenario: Invalid voice name
- **GIVEN** any state
- **WHEN** the user runs `--voice robot`
- **THEN** the system SHALL exit non-zero and list the valid values

### FR-008 Built-in voice (UR-002, Phase 1)
The system SHALL provide `--voice builtin`, using Chatterbox-Turbo's bundled default voice (`conds.pt`). The `voice:` line SHALL show `builtin`.

#### Scenario: Built-in voice
- **GIVEN** setup is complete
- **WHEN** the user runs `--voice builtin`
- **THEN** the `voice:` line SHALL show `builtin`
- **AND** the system SHALL generate audio with the bundled conditionals

#### Scenario: Built-in conditionals missing
- **GIVEN** the downloaded model lacks `conds.pt`
- **WHEN** the user runs `--voice builtin`
- **THEN** the system SHALL exit non-zero and advise running `narrate setup`

### FR-009 Custom voice clip (UR-002, Phase 1)
The system SHALL accept `--voice-file <wav>` as a reference clip. The system SHALL reject a clip of 5 s or shorter. When the clip is longer than 10 s, the system SHALL print, before generation, a notice that only the first 10 s are used; for a clip between 5 s and 10 s no notice is printed. The `voice:` line SHALL show the clip path. `--voice-file` and `--voice` together SHALL be rejected as conflicting.

#### Scenario: Valid long clip
- **GIVEN** a 12 s WAV file
- **WHEN** the user runs `--voice-file my.wav`
- **THEN** the `voice:` line SHALL show the path of `my.wav`
- **AND** the system SHALL print the notice that only the first 10 s are used

#### Scenario: Valid short clip
- **GIVEN** a 7 s WAV file
- **WHEN** the user runs `--voice-file my.wav`
- **THEN** the system SHALL generate audio and SHALL NOT print the first-10-s notice

#### Scenario: Short clip
- **GIVEN** a 4 s WAV file
- **WHEN** the user runs `--voice-file short.wav`
- **THEN** the system SHALL exit non-zero stating the clip must be longer than 5 s

#### Scenario: Clip missing
- **GIVEN** `gone.wav` does not exist
- **WHEN** the user runs `--voice-file gone.wav`
- **THEN** the system SHALL exit non-zero and state the path that was not found

#### Scenario: Not a WAV or corrupt
- **GIVEN** `fake.wav` is a text file renamed to `.wav`, or a truncated WAV
- **WHEN** the user runs `--voice-file fake.wav`
- **THEN** the system SHALL exit non-zero and name the file and say it is not a readable WAV

#### Scenario: Unsupported WAV format
- **GIVEN** a WAV file with an encoding the system cannot decode
- **WHEN** the user runs `--voice-file odd.wav`
- **THEN** the system SHALL exit non-zero, name the file, and list the supported WAV encodings

#### Scenario: Conflicting options
- **GIVEN** both `--voice male` and `--voice-file my.wav`
- **WHEN** the user runs the command
- **THEN** the system SHALL exit non-zero stating the options conflict

### FR-010 Setup command (UR-003, Phase 1)
`narrate setup` SHALL: create one or more isolated Python environments with uv (the split is an architect decision); install the official `chatterbox-tts` package, PyTorch, and their dependencies from the committed lockfile; download the Chatterbox-Turbo weights (`ResembleAI/chatterbox-turbo`) and `hexgrad/Kokoro-82M` at pinned commits; and generate the female, male, and builtin-voice reference clips, each at least 10 s long (the builtin clip serves `narrate-onnx` in Phase 2 and is produced from Chatterbox-Turbo's bundled voice). Re-running SHALL repair an incomplete setup without error. Re-running on a complete setup SHALL download nothing and leave the reference clips byte-identical.

#### Scenario: First setup
- **GIVEN** network access and no prior setup
- **WHEN** the user runs `narrate setup`
- **THEN** the environment, weights, and the female, male, and builtin reference clips SHALL exist in the data directory
- **AND** each clip SHALL be at least 10 s long
- **AND** `narrate status` SHALL report ready

#### Scenario: Re-run on complete setup
- **GIVEN** setup is complete
- **WHEN** the user runs `narrate setup` again
- **THEN** the system SHALL make no download requests
- **AND** all reference clips SHALL be byte-identical to before

#### Scenario: Setup fails midway
- **GIVEN** the network drops during weight download
- **WHEN** `narrate setup` fails
- **THEN** the system SHALL exit non-zero with the failing step named
- **AND** `narrate status` SHALL report not ready
- **AND** a re-run of `narrate setup` SHALL complete the setup

#### Scenario: uv missing
- **GIVEN** `uv` is not installed
- **WHEN** the user runs `narrate setup`
- **THEN** the system SHALL exit non-zero and state how to install uv

### FR-011 Official sources, pins, and hash verification (UR-010, Phase 1)
Setup SHALL fetch artifacts only from this allow-list, and SHALL verify every artifact against a pinned hash:
- PyPI packages, only through a committed lockfile that lists every package with its sha256 hash.
- Torch-family packages, only from `download.pytorch.org` through an explicit index.
- The uv-managed Python from `astral-sh/python-build-standalone` on GitHub.
- Hugging Face repos `ResembleAI/chatterbox-turbo` and `hexgrad/Kokoro-82M`, at pinned commit hashes.
- Kokoro grapheme-to-phoneme assets (the spaCy English model and espeak-ng), if needed, pinned and hash-verified.

The pin set SHALL also include the uv version and the python-build-standalone build URL and sha256. Setup SHALL refuse to run when the installed uv version differs from the pinned version, and SHALL print the install command for the pinned uv. Setup SHALL NOT download uv itself, so uv adds no allow-list source. The spec SHALL enumerate the hosts that serve these sources.

#### Scenario: Setup through an allow-list proxy
- **GIVEN** an HTTP proxy that permits only the allow-listed hosts and logs every request
- **WHEN** the user runs `narrate setup` through that proxy
- **THEN** setup SHALL succeed
- **AND** the proxy log SHALL show requests only to allow-listed hosts

#### Scenario: Disallowed source
- **GIVEN** the lockfile or pin list contains a URL outside the allow-list
- **WHEN** the user runs `narrate setup`
- **THEN** setup SHALL exit non-zero before downloading, naming the URL

#### Scenario: Pins recorded
- **GIVEN** the repository source
- **WHEN** a reviewer inspects the pin list and lockfile
- **THEN** every package SHALL have an exact version and sha256 hash and every Hugging Face repo SHALL have a full commit hash
- **AND** the uv version and the python-build-standalone build URL and sha256 SHALL be recorded

#### Scenario: uv version mismatch
- **GIVEN** the installed uv version differs from the pinned version
- **WHEN** the user runs `narrate setup`
- **THEN** setup SHALL exit non-zero before downloading anything, state both versions, and print the install command for the pinned uv

### FR-012 Offline generation (UR-003, Phase 1)
After setup, generation SHALL work with no network access and SHALL NOT contact any remote host. The test method SHALL need no admin rights: set `HTTP_PROXY` and `HTTPS_PROXY` to a closed local port, and observe the worker's connections. If the spec cannot make this check automatic, it SHALL be a documented manual Owner test.

#### Scenario: Offline run
- **GIVEN** setup is complete and the proxy variables point to a closed local port
- **WHEN** the user runs `narrate script.txt`
- **THEN** the system SHALL write the WAV file successfully
- **AND** the worker process SHALL hold no connection to a remote address during the run

#### Scenario: Weights missing while offline
- **GIVEN** the model weights were deleted and the proxy variables point to a closed local port
- **WHEN** the user runs `narrate script.txt`
- **THEN** the system SHALL exit non-zero advising `narrate setup`
- **AND** SHALL NOT attempt a download

### FR-013 Device selection (UR-004, Phase 1)
The system SHALL provide `--device auto|cuda|cpu`, default `auto`. `auto` SHALL use CUDA when available and SHALL print a warning when it falls back to CPU. `cuda` SHALL fail when CUDA is unavailable.

#### Scenario: Auto with GPU
- **GIVEN** a working NVIDIA GPU and driver
- **WHEN** the user runs with `--device auto`
- **THEN** generation SHALL run on CUDA and the system SHALL print the device used

#### Scenario: Auto without GPU
- **GIVEN** CUDA is unavailable
- **WHEN** the user runs with `--device auto`
- **THEN** generation SHALL run on CPU and the system SHALL print a warning about the fallback

#### Scenario: CUDA forced but unavailable
- **GIVEN** CUDA is unavailable
- **WHEN** the user runs with `--device cuda`
- **THEN** the system SHALL exit non-zero stating CUDA is unavailable and suggesting `--device cpu`

### FR-014 Status command (UR-006, Phase 1)
`narrate status` SHALL report whether setup is complete and, for each component (environment, package version, model weights, female clip, male clip, builtin clip), whether it is present. It SHALL report whether the data directory's recorded pin set matches the running binary's pin set (see FR-017). It SHALL also report whether CUDA is available.

#### Scenario: Ready
- **GIVEN** setup is complete
- **WHEN** the user runs `narrate status`
- **THEN** the system SHALL list each component as present, report the pin set as matching, report the CUDA state, and exit 0

#### Scenario: Not ready
- **GIVEN** the model weights are missing
- **WHEN** the user runs `narrate status`
- **THEN** the system SHALL mark the weights missing and exit non-zero

### FR-015 Generation before setup (UR-006, Phase 1)
When setup is incomplete, a generation command SHALL fail before any processing and tell the user to run `narrate setup`.

#### Scenario: Not set up
- **GIVEN** no setup has run
- **WHEN** the user runs `narrate script.txt`
- **THEN** the system SHALL exit non-zero with a message to run `narrate setup`

#### Scenario: Set up
- **GIVEN** setup is complete
- **WHEN** the user runs `narrate script.txt`
- **THEN** the system SHALL NOT print the setup message

### FR-016 Windows installer (UR-005, Phase 1)
The system SHALL ship as an Inno Setup `setup.exe` that installs `narrate` per-user under `%LOCALAPPDATA%\Programs`, adds the install directory to the user PATH, and requires no admin rights. Installing over an existing install SHALL leave exactly one PATH entry for the install directory. Uninstall SHALL remove the binary and the PATH entry.

#### Scenario: Install
- **GIVEN** a standard (non-admin) Windows user
- **WHEN** the user runs `setup.exe`
- **THEN** no elevation prompt SHALL appear
- **AND** `narrate --version` SHALL work in a newly opened terminal

#### Scenario: Reinstall
- **GIVEN** `narrate` is already installed
- **WHEN** the user runs `setup.exe` again
- **THEN** the user PATH SHALL contain the install directory exactly once

#### Scenario: Install location not writable
- **GIVEN** the current user has a deny-write ACL on `%LOCALAPPDATA%\Programs\narrate` (set with `icacls`)
- **WHEN** the user runs `setup.exe`
- **THEN** the installer SHALL report the failure, exit non-zero, and leave no PATH entry

#### Scenario: Uninstall
- **GIVEN** `narrate` is installed
- **WHEN** the user uninstalls it
- **THEN** the install directory and the user PATH entry SHALL be removed

### FR-017 Shared data directory and pin-set record (UR-003, Phase 1)
The Python environment(s), model weights, and voice clips SHALL live in the data directory, default `%LOCALAPPDATA%\narrate`, shared by development builds and the installed binary. The environment variable `NARRATE_DATA_DIR` SHALL override the location. Setup SHALL record in the data directory the pin set it was built with. When the recorded pin set differs from the running binary's pin set, `narrate status` SHALL report the mismatch and generation SHALL fail, both pointing to `narrate setup`.

#### Scenario: Dev build reuses setup
- **GIVEN** setup was run by the installed binary
- **WHEN** a development build with the same pin set runs `narrate status`
- **THEN** the system SHALL report ready without a second setup

#### Scenario: Override
- **GIVEN** `NARRATE_DATA_DIR` points to an empty scratch directory
- **WHEN** the user runs `narrate status`
- **THEN** the system SHALL report not ready, and the default directory SHALL be untouched

#### Scenario: Pin-set mismatch
- **GIVEN** a data directory built with a different pin set than the running binary
- **WHEN** the user runs `narrate status` or generates audio
- **THEN** the system SHALL report the mismatch and point to `narrate setup`
- **AND** generation SHALL exit non-zero

#### Scenario: Missing directory
- **GIVEN** the data directory does not exist
- **WHEN** the user runs `narrate status`
- **THEN** the system SHALL report not ready and exit non-zero

### FR-018 Watermark in narrate (UR-001, Phase 1)
The system SHALL leave the PerTh watermark that `chatterbox-tts` embeds enabled on all `narrate` output, and SHALL NOT write unwatermarked output.

#### Scenario: Watermark present
- **GIVEN** a generated WAV file
- **WHEN** the PerTh extractor is run on it
- **THEN** a watermark SHALL be detected

#### Scenario: No disable option
- **GIVEN** the CLI help
- **WHEN** the user lists options
- **THEN** no option to disable the watermark SHALL exist

#### Scenario: Watermarker unavailable
- **GIVEN** the test hook makes the PerTh component fail to load
- **WHEN** the user generates audio
- **THEN** the system SHALL exit non-zero and SHALL NOT write an output file

### FR-024 Global options (UR-006, Phase 1)
The system SHALL support `--version` (prints the version, exit 0), `--help` (prints usage, exit 0), and `--verbose` (prints detailed progress and stack traces on error) on `narrate`. An unknown option SHALL exit non-zero with usage.

#### Scenario: Version and help
- **GIVEN** `narrate` is installed
- **WHEN** the user runs `narrate --version` and `narrate --help`
- **THEN** each SHALL print its output and exit 0

#### Scenario: Verbose error
- **GIVEN** a generation that fails
- **WHEN** the user adds `--verbose`
- **THEN** the system SHALL print the underlying error details that are hidden without it

#### Scenario: Unknown option
- **GIVEN** any state
- **WHEN** the user runs `narrate --bogus`
- **THEN** the system SHALL exit non-zero and print usage

### Phase 2

### FR-019 ONNX binary generation and voices (UR-008, Phase 2)
The system SHALL provide a separate binary `narrate-onnx` that accepts the same input files, `--voice`, `--voice-file`, `-o`, and `--device` options as `narrate`, and runs the official `ResembleAI/chatterbox-turbo-ONNX` export through ONNX Runtime in Rust with no Python. `narrate-onnx` SHALL read the female and male reference clips from the shared data directory, and therefore needs a completed `narrate setup` as well as `narrate-onnx setup`. `--voice builtin` SHALL work in `narrate-onnx`; the builtin conditionals come from a file the pinned ONNX repo ships if one exists, else from the builtin reference clip that `narrate setup` already produces (FR-010), so `narrate` does not change in Phase 2 (the architect chooses the mechanism). `narrate-onnx` SHALL support `--version`, `--help`, `--verbose`, and the unknown-option behavior of FR-024, and SHALL provide the same test hooks as FR-005. FR-001 to FR-006 and FR-009 apply to `narrate-onnx` as they apply to `narrate`.

#### Scenario: ONNX generation
- **GIVEN** the ONNX model is downloaded and `narrate setup` is complete
- **WHEN** the user runs `narrate-onnx script.txt`
- **THEN** the system SHALL write a WAV file and SHALL NOT start a Python process

#### Scenario: Built-in voice in ONNX
- **GIVEN** the ONNX model is downloaded and `narrate setup` is complete
- **WHEN** the user runs `narrate-onnx script.txt --voice builtin`
- **THEN** the system SHALL generate audio and the `voice:` line SHALL show `builtin`

#### Scenario: Global options in ONNX
- **GIVEN** `narrate-onnx` is installed
- **WHEN** the user runs `narrate-onnx --version`, `narrate-onnx --help`, and `narrate-onnx --bogus`
- **THEN** the first two SHALL print output and exit 0, and the last SHALL exit non-zero and print usage

#### Scenario: Model not downloaded
- **GIVEN** the ONNX model is absent
- **WHEN** the user runs `narrate-onnx script.txt`
- **THEN** the system SHALL exit non-zero and tell the user to run `narrate-onnx setup`

#### Scenario: Reference clips missing
- **GIVEN** the female and male clips are absent from the data directory
- **WHEN** the user runs `narrate-onnx script.txt`
- **THEN** the system SHALL exit non-zero and tell the user to run `narrate setup`

### FR-020 ONNX setup and status (UR-008, UR-010, Phase 2)
`narrate-onnx setup` SHALL download the ONNX model only from Hugging Face `ResembleAI/chatterbox-turbo-ONNX` at a pinned commit hash, SHALL verify every file against a pinned sha256 hash, and generation SHALL work offline afterward. `narrate-onnx status` SHALL report whether the model files, the reference clips (female, male, builtin), and the ONNX Runtime are present, SHALL report the ONNX Runtime version and the path of the loaded ONNX Runtime DLL, and which execution providers are available; it SHALL exit non-zero when not ready.

#### Scenario: Pinned download
- **GIVEN** network access
- **WHEN** the user runs `narrate-onnx setup`
- **THEN** the files SHALL come from the pinned revision of the official repo and match their pinned hashes

#### Scenario: Offline after download
- **GIVEN** the model is downloaded and the proxy variables point to a closed local port
- **WHEN** the user runs `narrate-onnx script.txt`
- **THEN** generation SHALL succeed

#### Scenario: Download failure
- **GIVEN** the network drops during `narrate-onnx setup`
- **WHEN** the setup fails
- **THEN** the system SHALL exit non-zero naming the failing file, `narrate-onnx status` SHALL report not ready, and a re-run SHALL complete the setup

#### Scenario: Hash mismatch
- **GIVEN** a downloaded file does not match its pinned hash
- **WHEN** `narrate-onnx setup` verifies it
- **THEN** setup SHALL exit non-zero naming the file and SHALL discard it

#### Scenario: Status
- **GIVEN** the model and clips are present
- **WHEN** the user runs `narrate-onnx status`
- **THEN** the system SHALL list each component as present, show the ONNX Runtime version, the loaded DLL path, and the execution providers, and exit 0

### FR-021 ONNX GPU use (UR-008, Phase 2)
`narrate-onnx` SHALL use the GPU when available under `--device auto` and SHALL warn when it falls back to CPU, matching FR-013 behavior.

#### Scenario: GPU available
- **GIVEN** a working NVIDIA GPU
- **WHEN** the user runs `narrate-onnx script.txt`
- **THEN** the system SHALL report the GPU execution provider in use

#### Scenario: GPU unavailable
- **GIVEN** no usable GPU provider
- **WHEN** the user runs with `--device auto`
- **THEN** the system SHALL run on CPU and print a fallback warning

### FR-022 Phase 1 binary unchanged (UR-008, Phase 2)
Adding `narrate-onnx` SHALL NOT change the behavior, options, or dependencies of `narrate`.

#### Scenario: Regression check
- **GIVEN** the Phase 1 acceptance tests
- **WHEN** they run after `narrate-onnx` is added
- **THEN** all SHALL pass without modification

#### Scenario: Dependency check
- **GIVEN** the `narrate` build manifest
- **WHEN** compared with its Phase 1 version
- **THEN** it SHALL contain no ONNX Runtime dependency

#### Scenario: Regression detected
- **GIVEN** a Phase 2 change that breaks a Phase 1 acceptance test
- **WHEN** the suite runs
- **THEN** the suite SHALL fail and the Phase 2 change SHALL NOT be accepted

### FR-023 Comparison report and tool (UR-009, Phase 2)
A comparison tool, implemented as a repo script (not a `narrate` subcommand), SHALL produce a report comparing `narrate` and `narrate-onnx` on the same machine, script, voice, and device. Metrics are defined as:
- Real-time factor (RTF): synthesis time divided by audio duration, excluding model load.
- Model load time: time from process start to ready for the first chunk.
- Peak VRAM: peak device memory used over an idle baseline, sampled through NVML for both backends.
- Disk size: total size of every file each binary loads at run time (model files, Python environment including PyTorch's CUDA libraries for `narrate`; ONNX Runtime and CUDA execution-provider DLLs for `narrate-onnx`), excluding caches.
- Output pitch and duration: median F0 and duration of the output.
- Dependency surface: package or crate count, number of distinct download sources, and install size.
- Watermark: whether the output carries the PerTh watermark (see FR-025).

#### Scenario: Report complete
- **GIVEN** both backends are installed
- **WHEN** the comparison is run
- **THEN** the report SHALL contain a measured value for every listed metric for both backends, with the script, voice, and device stated

#### Scenario: Backend missing
- **GIVEN** one backend is not installed
- **WHEN** the comparison is run
- **THEN** the system SHALL exit non-zero naming the missing backend

### FR-025 ONNX watermark (UR-001, UR-009, Phase 2)
`narrate-onnx` SHALL embed the PerTh watermark if an official ONNX or Rust port of PerTh exists, as determined by a research step recorded in the comparison report. Otherwise its output SHALL be unwatermarked, and the comparison report SHALL state that difference.

#### Scenario: Port exists
- **GIVEN** the research step found an official PerTh ONNX or Rust port
- **WHEN** `narrate-onnx` generates audio
- **THEN** the PerTh extractor SHALL detect a watermark in the output

#### Scenario: No port exists
- **GIVEN** the research step found no official port
- **WHEN** the comparison report is produced
- **THEN** the report SHALL state that `narrate-onnx` output is unwatermarked

#### Scenario: Port exists but fails to load
- **GIVEN** the research step found a port and the test hook makes the port fail to load
- **WHEN** `narrate-onnx` generates audio
- **THEN** the system SHALL exit non-zero and SHALL NOT write an output file

### FR-026 ONNX installer (UR-005, UR-010, Phase 2)
The installer SHALL also install `narrate-onnx` in the install directory together with the ONNX Runtime DLLs and the CUDA execution-provider DLLs, taken from named official sources at pinned versions and verified by sha256 (the spec names them, for example the official ONNX Runtime release on GitHub and NVIDIA's official redistributables). `narrate-onnx` SHALL load the ONNX Runtime from its own install directory.

#### Scenario: Correct runtime loaded
- **GIVEN** `narrate-onnx` is installed and an older `onnxruntime.dll` exists in `System32`
- **WHEN** the user runs `narrate-onnx status`
- **THEN** the reported ONNX Runtime version SHALL equal the pinned version and the loaded path SHALL be in the install directory

#### Scenario: Hash mismatch at build
- **GIVEN** a runtime DLL whose hash differs from the pinned hash
- **WHEN** the installer is built
- **THEN** the build SHALL fail naming the file

## 5. Non-functional requirements

| ID | Statement | Phase | Traces to |
|---|---|---|---|
| NFR-001 | Every Python artifact SHALL be pinned to an exact version with a sha256 hash in the committed lockfile, and every Hugging Face file SHALL be checked against the pinned commit; no floating tags or ranges. | 1, 2 | UR-010 |
| NFR-002 | Every command (setup, generation workers, and status) SHALL NOT modify system Python, global site-packages, or any path outside the data directory, except the output WAV file and its temporary file in the output directory (FR-006). Every command SHALL redirect every tool cache and install location (`UV_CACHE_DIR`, `UV_PYTHON_INSTALL_DIR`, uv's Python bin directory `%USERPROFILE%\.local\bin`, `HF_HOME` or equivalent) into the data directory, and SHALL NOT register Python under `HKCU\Software\Python` (PEP 514). | 1, 2 | UR-010 |
| NFR-003 | The installer and all commands SHALL run without administrator rights. | 1, 2 | BR-002 |
| NFR-004 | Female output SHALL have a median F0 of at least 165 Hz and male output SHALL have a median F0 of at most 155 Hz. The spec SHALL fix the test script, the F0 estimator, and the voiced-frame rule. Only the Owner may change these thresholds. Builtin voice median F0 SHALL be measured and reported with no pass/fail threshold. | 1 | UR-002 |
| NFR-005 | Generation of a 10,000-word script SHALL complete on an 8 GB GPU without running out of memory. If the joined audio would exceed the 4 GiB WAV size limit, the system SHALL stop with an error stating the limit and SHALL NOT leave an output file (tested by lowering the limit through the FR-005 test hook). | 1 | UR-004 |
| NFR-006 | On CUDA, generation SHOULD run faster than real time (RTF below 1.0). The target is an estimate to validate on the RTX 3050. | 1, 2 | UR-004 |
| NFR-007 | Every error message SHALL state the cause and the next action (command or fix), and SHALL NOT show a raw stack trace unless `--verbose` is set. | 1, 2 | UR-006 |
| NFR-008 | Every command SHALL exit 0 on success and non-zero on failure. | 1, 2 | UR-006 |
| NFR-009 | The tools SHALL run on Windows 11 x64. | 1, 2 | BR-002 |
| NFR-010 | Comparison metrics SHALL be measured on the same machine, script, voice, and device, and each result SHALL state the commit and versions used. | 2 | BR-004 |
| NFR-011 | Voice quality SHALL be accepted by the Owner listening to the default female output; this acceptance is manual. | 1 | UR-002 |
| NFR-012 | The measured RTF SHALL be recorded in the Phase 1 QA report and, in Phase 2, in the comparison report. | 1, 2 | UR-004, BR-004 |

### NFR scenarios

#### Scenario: Hash mismatch fails setup (NFR-001)
- **GIVEN** a downloaded Python artifact or Hugging Face file whose hash differs from the pinned value
- **WHEN** `narrate setup` verifies it
- **THEN** setup SHALL exit non-zero naming the file and SHALL NOT install or use it

#### Scenario: Nothing changes outside the data directory (NFR-002)
- **GIVEN** snapshots of `%LOCALAPPDATA%\uv`, `%APPDATA%\uv`, `%USERPROFILE%\.local\bin`, `%USERPROFILE%\.cache\huggingface`, `HKCU\Software\Python`, and the user PATH
- **WHEN** `narrate setup` and then `narrate script.txt` run with `NARRATE_DATA_DIR` set to a scratch directory
- **THEN** those locations, the registry key, and the user PATH SHALL be unchanged
- **AND** the uv cache, uv Pythons, and Hugging Face cache SHALL exist inside the scratch directory

## 6. Open points for the architect

- Whether uninstall also removes `%LOCALAPPDATA%\narrate` (large; may be shared with dev builds). Requirements fix only binary and PATH removal.
- Chunk size limit, pause lengths, the exact pinned versions and hashes, and the enumerated allow-list hosts are design decisions.
- The Markdown element rules in FR-002 and the overwrite rule in FR-006 are defaults chosen here; the Owner may revise them.
- The builtin-voice mechanism for `narrate-onnx` (FR-019): a file shipped by the pinned ONNX repo, or the builtin reference clip that `narrate setup` produces in Phase 1 (FR-010).

## 7. Traceability

| BR | UR | FR / NFR |
|---|---|---|
| BR-001 | UR-001, 002, 003, 004, 006, 007 | FR-001 to 010, 012 to 015, 017, 018, 024, 025; NFR-004, 005, 006 to 008, 011 |
| BR-002 | UR-005, 006 | FR-016, 024, 026; NFR-003, 009 |
| BR-003 | UR-010 | FR-011, 020, 026; NFR-001, 002 |
| BR-004 | UR-008, 009 | FR-019 to 023, 025; NFR-010, 012 |

NFR-006 traces to UR-004. NFR-012 traces to UR-004 and BR-004.

## 8. Self-check

- Every FR traces to a UR and every UR to a BR; no orphans.
- Every FR has a happy-path and a failure-path scenario.
- Each FR and NFR carries a phase. Each requirement uses one binding keyword.
- Implementation detail is limited to decisions fixed by the Owner (uv, Inno Setup, Kokoro voices, repo names, allow-list sources).
