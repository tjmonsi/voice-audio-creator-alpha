# narrate

`narrate` turns a Markdown or text script into a narrated WAV file (24 kHz, mono, 16-bit) on your own machine. It runs the official [Chatterbox-Turbo](https://huggingface.co/ResembleAI/chatterbox-turbo) model from the `chatterbox-tts` Python package inside a private, version-pinned Python environment that `narrate setup` builds for you. Nothing is sent to a service after setup.

```text
narrate story.md                      # writes story.wav next to the script
narrate story.md --voice male -o out\story.wav
```

## Requirements

| Item | Needed for | Notes |
|---|---|---|
| Windows 11 x64 | everything | The installer and all pins target `x86_64-pc-windows-msvc` |
| [uv](https://docs.astral.sh/uv/) 0.10.2 | `narrate setup` | Exactly this version. `setup` never downloads uv itself; it prints the install command if uv is missing or has another version |
| About 6.2 GB free disk space (estimate) | `narrate setup` | Python, the locked environment, and the model files, all inside the data directory |
| NVIDIA GPU, 8 GB VRAM or more (recommended) | fast generation | Without CUDA, `--device auto` falls back to the CPU and warns that it is slower |
| Network | `narrate setup` only | After a complete setup, generation makes no network request |

Install uv 0.10.2 from the pinned release (check the hash before using it):

```text
https://github.com/astral-sh/uv/releases/download/0.10.2/uv-x86_64-pc-windows-msvc.zip
sha256 493ebbe0e06128d6ee4905e1ed5e2a433fb0f7cfc08b0eaca9fab4ca76778ae1
```

Put `uv.exe` from the zip on your `PATH`. Astral's installer also works: `powershell -ExecutionPolicy ByPass -c "irm https://astral.sh/uv/0.10.2/install.ps1 | iex"`. That script is not hash-pinned.

## Install

Run `dist\narrate-setup-0.1.0.exe` (built as described under [Build](#build-from-source)). The installer needs no administrator rights. It copies `narrate.exe` to `%LOCALAPPDATA%\Programs\narrate` and adds that folder to your user `PATH`. Open a new terminal afterwards so the new `PATH` is picked up.

## Quick start

```text
narrate setup                  # once: installs Python, the environment, models, and voice clips
narrate status                 # shows what is ready and whether CUDA is visible
narrate story.md               # writes story.wav
```

`narrate setup` is safe to run again. Each step skips work that is already complete and verified, so a rerun after a network failure continues where it stopped. A rerun on a complete setup makes no network request.

## Usage

```text
narrate [--verbose] <FILE> [-o <PATH>] [--voice <VOICE> | --voice-file <WAV>] [--device <DEVICE>]
narrate [--verbose] setup
narrate [--verbose] status
```

| Option | Values | Default | Meaning |
|---|---|---|---|
| `<FILE>` | `.md`, `.markdown`, `.txt` (any letter case) | required | Script to narrate. UTF-8, with or without a BOM |
| `-o`, `--output` | path | `<FILE>` with the extension replaced by `.wav` | Output file. The directory must exist |
| `--voice` | `female`, `male`, `builtin` | `female` | Preset voice |
| `--voice-file` | path to a WAV | none | Clone any reference clip. It must be longer than 5 s, and only the first 10 s are used |
| `--device` | `auto`, `cuda`, `cpu` | `auto` | `auto` uses CUDA when available and warns before falling back to the CPU. `cuda` fails if CUDA is missing |
| `--verbose` | flag | off | Prints chunk text, worker output, sample counts, and error details |

`narrate --help` lists everything. A script named `setup.md` is treated as a file because only the exact word `setup` is the command.

On success, stdout is exactly the absolute path of the WAV and every progress line goes to stderr, so `$wav = narrate story.md` works in scripts. The `done:` line reports audio length, synthesis time, and the real-time factor (rtf, synthesis seconds per audio second).

### Voices

| Voice | Source |
|---|---|
| `female` | Clone of a clip that `setup` renders once with Kokoro-82M `af_heart` |
| `male` | Clone of a clip that `setup` renders once with Kokoro-82M `am_michael` |
| `builtin` | The voice bundled with Chatterbox-Turbo (`conds.pt`) |
| `--voice-file` | Your own clip |

The clips live in `%LOCALAPPDATA%\narrate\voices` and are never re-rendered unless you delete one or the pin set changes.

### What the script converter does

| Input | Spoken as |
|---|---|
| Headings | The heading text, followed by a 0.8 s pause |
| Paragraphs and list items | The text, followed by a 0.5 s pause |
| Table cells | One cell at a time, 0.3 s between cells and 0.5 s after the row |
| Emphasis, links, images, inline code | The visible text (image alt text, link text, code content) |
| Fenced and indented code blocks, front matter, HTML comments, footnote definitions | Skipped |
| `.txt` files | Paragraphs split at blank lines. Nothing else is changed |

Long paragraphs are split at sentence boundaries into chunks of at most 300 characters, with a 0.2 s pause between chunks. If any chunk fails, `narrate` stops and writes no output; an existing output file stays untouched.

Chatterbox-Turbo reads these tags when you write them in square brackets, in lowercase: `[angry]`, `[fear]`, `[surprised]`, `[whispering]`, `[advertisement]`, `[dramatic]`, `[narration]`, `[crying]`, `[happy]`, `[sarcastic]`, `[clear throat]`, `[sigh]`, `[shush]`, `[cough]`, `[groan]`, `[sniff]`, `[gasp]`, `[chuckle]`, `[laugh]`. Other bracketed words are read as text, and `narrate` warns once per distinct unknown tag.

### Watermark

Every generated file carries Resemble AI's PerTh watermark. There is no option to turn it off, and `narrate` refuses to run if the watermarker cannot load.

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Generation failed (worker, model, device, watermark, size limit, final write) |
| 2 | Usage error |
| 3 | Problem with the input, output, or voice clip |
| 4 | Not set up, or set up by a build with different pins (run `narrate setup`) |
| 5 | Setup failed |
| 130 | Interrupted with Ctrl+C or Ctrl+Break |

Every error prints `error[<code>]: <cause>` followed by a `fix:` line. Add `--verbose` for the underlying details.

## Where things live

Everything `narrate` installs is in one data directory, `%LOCALAPPDATA%\narrate`. Set `NARRATE_DATA_DIR` to use another folder (installed and development builds that share a folder share one setup).

| Path | Contents |
|---|---|
| `python\`, `python-cache\` | CPython 3.11.14 from python-build-standalone, and its verified archive |
| `venv\`, `uv-cache\` | The locked Python environment and uv's cache |
| `models\` | Chatterbox-Turbo and Kokoro-82M files, each verified against a pinned sha256 |
| `voices\` | `female.wav`, `male.wav`, `builtin.wav` |
| `setup.json` | The pin set that built this directory |
| `logs\` | `setup-last.log`, and `worker-<pid>.log` for failed runs |

`setup` redirects every cache and temporary file of uv, Hugging Face, torch, and the worker into this directory. Outside it, `narrate` changes only the output WAV, the user `PATH` entry the installer adds, and the installer's uninstall entry.

## Security and pins

`setup` downloads only from these sources, each pinned and hash-verified:

| Source | What | How it is pinned |
|---|---|---|
| PyPI (`pypi.org`, `files.pythonhosted.org`) | Python packages | `python\uv.lock` lists the sha256 of every file; `uv sync --frozen` enforces it |
| `download.pytorch.org` | `torch` and `torchaudio` only (CUDA 12.4 builds) | Same lock file |
| GitHub release assets | The spaCy `en_core_web_sm` 3.8.0 wheel; python-build-standalone CPython 3.11.14 | Wheel hash in the lock; archive sha256 in `pins.json`, checked by `narrate` |
| Hugging Face (`ResembleAI/chatterbox-turbo`, `hexgrad/Kokoro-82M`) | 13 model files | Full 40-character commit and per-file size and sha256 in `pins.json`, checked by `narrate` |

Before it creates anything, `setup` scans `uv.lock` and `pins.json` and stops with exit code 5 if a URL is outside this list or an artifact has no hash. The pins, the lock, and the worker script are compiled into the binary, so the binary alone defines the pin set (the `pin set:` line of `narrate status`).

## Uninstall

Use Windows Settings, Installed apps, or run `%LOCALAPPDATA%\Programs\narrate\unins000.exe`. The uninstaller removes `narrate.exe` and its `PATH` entry. It keeps the data directory because it holds several GB that development builds share. To delete it by hand:

```powershell
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\narrate"
```

## Build from source

Requirements: Rust 1.99 (edition 2024), and for the installer [Inno Setup](https://jrsoftware.org/isinfo.php) 6.7.3 (`winget install --id JRSoftware.InnoSetup --exact --version 6.7.3 --scope user`).

```powershell
cargo build --release -p narrate        # target\release\narrate.exe
powershell -NoProfile -ExecutionPolicy Bypass -File installer\build.ps1   # dist\narrate-setup-0.1.0.exe
```

`build.ps1` refuses a binary that prints anything other than `narrate <version>` for `--version`, so a test build is never packaged.

### Tests

```powershell
cargo test --workspace --all-features
```

The `test-hooks` feature adds environment overrides (`NARRATE_WORKER_CMD`, `NARRATE_UV`, `NARRATE_TEST_*`) that the black-box tests use. Release builds ignore them. The automated tests need no GPU, network, uv, or Python. The GPU and installer checks are listed in `specs\spec-narrate-test-strategy.md`.

### Layout

```text
crates/narrate-core    text pipeline, WAV encoding, reference-clip helpers (library)
crates/narrate         the command-line program, pins.json
crates/narrate-testkit fake worker, fake uv, proxy, and signal helper for the tests
python/                pyproject.toml, uv.lock, and narrate_worker.py (the single-file worker)
installer/             narrate.iss and build.ps1
specs/                 the specification the code follows
```

To change a pin, edit `crates/narrate/pins.json` or `python/pyproject.toml`, then re-lock with uv 0.10.2: `uv lock --project python --exclude-newer <date>`. The PyTorch index lists its wheels under `download-r2.pytorch.org`; `narrate setup` allows only `download.pytorch.org`, so after re-locking replace `https://download-r2.pytorch.org/whl/` with `https://download.pytorch.org/whl/` in `python/uv.lock` (the files and their sha256 values are the same; `uv lock --check` accepts the edited lock). `narrate setup` rejects the lock with `error[source-not-allowed]` otherwise.

## Phase 2

`narrate-onnx`, an experimental ONNX Runtime backend, is planned as a separate binary in this repository. `narrate` stays unchanged until its output is compared with the Python backend. See `DECISIONS.md`.
