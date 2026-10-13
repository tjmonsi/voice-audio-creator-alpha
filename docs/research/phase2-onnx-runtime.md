# Phase 2 research: `narrate-onnx` (Rust + ONNX Runtime)

Checked 2026-10-12. Labels as in `phase1-python-runtime.md`: **tested**, **code-read**, **doc**, **unverified**. Graph facts below come from parsing the `.onnx` protobuf headers (downloaded at the pinned commit, sha256 matched the HF LFS hashes) with a small wire-format reader; the large `.onnx_data` weight files were not downloaded.

## Findings that change the design

| # | Finding | Evidence | Action |
|---|---|---|---|
| G1 | No official Resemble AI ONNX export or Rust port of the PerTh watermarker exists. | Section 9. | Per the user's rule, `narrate-onnx` does not watermark. |
| G2 | `ort` has no stable 2.x release; the newest is `2.0.0-rc.13` (2026-07-28, MSRV 1.88). Its default `download-binaries` feature fetches ONNX Runtime 1.28.0 builds made by pyke.io from `cdn.pyke.io`, not Microsoft artifacts. | crates.io API; `ort-sys/build/download/dist.tsv` at tag `v2.0.0-rc.13`; ort docs `misc/prebuilt-binaries.mdx` ("download statically-linked ONNX Runtime binaries built by pyke.io"). | For the official-sources rule: `ort = "=2.0.0-rc.13"` with `default-features = false` and `load-dynamic`, loading Microsoft's `onnxruntime.dll` from the official GitHub release by absolute path. |
| G3 | ORT's CUDA EP needs CUDA and cuDNN DLLs that the official ORT zip does not contain. torch's bundled CUDA 12.4 DLLs from phase 1 do not qualify: ORT 1.21+ CUDA-12 builds "require CUDA 12.8 or newer". | tested: zip listing of `onnxruntime-win-x64-gpu_cuda12-1.28.3.zip`; doc: https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html | Provision CUDA 13 (or 12.8+) runtime, cuBLAS, cuFFT, cuRAND and cuDNN 9 DLLs from NVIDIA (section 7.4). |
| G4 | ORT's CUDA kernel for `com.microsoft::GroupQueryAttention` accepts only float16/bfloat16. The fp32, q4 and q8 (`_quantized`) language models run GQA in float32, so those 24 attention nodes fall back to the CPU EP under CUDA. | doc: `docs/OperatorKernels.md` at ORT tag v1.28.3 (CUDA row: `T = tensor(bfloat16), tensor(float16)`); code-read: fp32 LM has float32 KV inputs; fp16 LM casts `inputs_embeds` to fp16 and stores fp16 weights and KV. | On CUDA use `language_model_fp16` or `language_model_q4f16`. Confirm node placement with ORT verbose logging. |
| G5 | Windows 11 ships `C:\Windows\System32\onnxruntime.dll` version 1.17 (this machine: `1.17.260613-0100.1.os-germanium.4acc1d4`). With `load-dynamic` and no `ORT_DYLIB_PATH`, ort tries `<exe dir>\onnxruntime.dll`, then a bare `LoadLibrary("onnxruntime.dll")`, which finds System32. ort then refuses any DLL whose minor version is below its API level (27 by default). | tested: PowerShell file version; code-read: ort `src/lib.rs` lines 122-170, 215-230 at rc.13. | Always call `ort::init_from(<absolute path>)` before any other ort call. |
| G6 | The ONNX tokenizer appends two `<|endoftext|>` (50256) tokens, and `embed_tokens` treats the last two input positions as speech tokens, mapping 50256 to 6561 (start-of-speech). | code-read: `tokenizer.json` post_processor; `embed_tokens.onnx` nodes and constants; tested: Rust `tokenizers` output in section 4. | Encode with `add_special_tokens = true` and feed the ids unchanged. |
| G7 | The official README loop decodes greedily (argmax + repetition penalty 1.2, max 1024 new tokens). The PyTorch package samples (temperature 0.8, top_k 1000, top_p 0.95, repetition penalty 1.2, max 1000). | code-read: ONNX README lines 110-160; `tts_turbo.py` lines 248-259, `t3.py` lines 415-426. | Pick one for the comparison and document it; sampling matches phase 1. |

## 1. Repository

`ResembleAI/chatterbox-turbo-ONNX`, commit `d21799bd0354adb85e348b8a0442a8405110a2cf` (lastModified 2025-12-15), license MIT (model card front matter `license: mit`; `base_model: ResembleAI/chatterbox-turbo`), not gated. Download URL form (tested): `https://huggingface.co/ResembleAI/chatterbox-turbo-ONNX/resolve/<sha>/onnx/<file>`; it answers with a redirect to a Hugging Face CDN host, so the client must follow cross-host redirects.

Small files (sha256 computed from downloads at the pinned commit):

| File | Bytes | sha256 | Content |
|---|---:|---|---|
| `tokenizer.json` | 3562272 | `3f04e34bea22f9144d1a19151154095bc9ce0430bf421304f5797e716288a906` | section 4 |
| `tokenizer_config.json` | 414 | `0d637373c70a54c3c7202c0c12b40ff4f346c329960283f5a88031717d73c66f` | GPT2Tokenizer, `model_max_length` 1024 |
| `config.json` | 1188 | `6b52ed838c3bc347ef519659d82fbd35b9ee805fc7a6da92fc46bc58dfa08770` | GPT-2: `n_layer` 24, `n_head` 16, `n_embd` 1024, `n_positions` 8196, `vocab_size` 6563; `transformers.js_config.kv_cache_dtype` fp16 for `fp16` and `q4f16` |
| `generation_config.json` | 55 | `91d2e5ff0fd172d5e8ac4818ef8bcd9b9ee4f7e562831edc8cc3b3702e64f923` | `{"eos_token_id": 6562, "repetition_penalty": 1.2}` (no temperature/top_k/top_p) |
| `preprocessor_config.json` | 130 | `1c320b2eea5f45678d17d891d51c38467775091dbf26a074635ba5e98ede5950` | `sampling_rate: 24000` |

## 2. Graph inputs and outputs (all precision variants share these names)

| Graph | Inputs | Outputs |
|---|---|---|
| `speech_encoder` | `audio_values` float32 `[batch, num_samples]` (24 kHz mono waveform) | `audio_features` float32 `[batch, seq, 1024]` (README name `cond_emb`); `audio_tokens` int64 `[batch, audio_seq]` (README `prompt_token`); `speaker_embeddings` float32 `[batch, 192]`; `speaker_features` float32 `[batch, feature_dim, 80]` |
| `embed_tokens` | `input_ids` int64 `[batch, seq]` | `inputs_embeds` float32 `[batch, seq, 1024]` |
| `language_model` | `inputs_embeds` float32 `[batch, seq, 1024]`; `attention_mask` int64 `[batch, total_seq]`; `position_ids` int64 `[batch, seq]`; `past_key_values.{0..23}.{key,value}` `[batch, 16, past_seq, 64]` | `logits` float32 `[batch, seq, 6563]`; `present.{0..23}.{key,value}` `[batch, 16, total_seq, 64]` |
| `conditional_decoder` | `speech_tokens` int64 `[batch, n_tokens]`; `speaker_embeddings` float32 `[batch, 192]`; `speaker_features` float32 `[batch, feature_dim, 80]` | `waveform` float32 `[batch, num_samples]` (24 kHz) |

- KV dtype: float32 in `language_model`, `_q4`, `_quantized`; float16 in `_fp16` and `_q4f16`. All other graph inputs and outputs stay float32/int64 in every variant (tested for all 20 graphs).
- Opsets and custom ops: speech_encoder and conditional_decoder use `com.microsoft::MultiHeadAttention`; language_model uses `com.microsoft::GroupQueryAttention` (24 nodes); q4/q4f16/quantized variants add `com.microsoft::MatMulNBits` and `GatherBlockQuantized`. conditional_decoder contains `RandomNormalLike`/`RandomUniformLike` (output is not deterministic) and STFT. Opsets: ai.onnx 20 (speech_encoder, embed_tokens), 21 (language_model), 17 (conditional_decoder).
- Internals: `embed_tokens` holds `text_emb.weight [50276, 1024]` and `speech_emb.weight [6563, 1024]`; it slices `input_ids[:, 0:-2]` into `text_emb` and `input_ids[:, -2:]` into `speech_emb` after `Where(id == 50256, 6561, id)`, then concatenates. `language_model` holds `transformer.wpe.weight [8196, 1024]` (absolute position embeddings gathered by `position_ids`) and `lm_head` to 6563 logits; `seqlens_k` for GQA is derived as `sum(attention_mask) - 1`. `speech_encoder` contains STFT ops (feature extraction is inside the graph) and the integer constant 240000 (10 s at 24 kHz; inferred to be the decoder-conditioning cap, matching PyTorch `DEC_COND_LEN`).
- External data: each `X.onnx` references exactly one file named `X.onnx_data` (tested for all 20). The ONNX spec defines `location` as "file path relative to the filesystem directory where the ONNX protobuf model was stored" (https://github.com/onnx/onnx/blob/main/docs/ExternalData.md). Keep the original file names side by side and load sessions from the file path (ort `commit_from_file`).

## 3. Generation loop (official README, `README.md` lines 61-186 at the pinned commit)

Constants: `SAMPLE_RATE = 24000`, `START_SPEECH_TOKEN = 6561`, `STOP_SPEECH_TOKEN = 6562`, `SILENCE_TOKEN = 4299`, `NUM_KV_HEADS = 16`, `HEAD_DIM = 64`, `max_new_tokens = 1024`, `repetition_penalty = 1.2`, `apply_watermark = False`.

1. `audio_values = librosa.load(voice_path, sr=24000)` as float32 `[1, N]`.
2. `input_ids = tokenizer(text)` as int64 `[1, T + 2]` (two trailing 50256 tokens, section 4).
3. `generate_tokens = [[6561]]`.
4. For `i` in `0..1024`:
   - `inputs_embeds = embed_tokens(input_ids)`.
   - If `i == 0`: run `speech_encoder(audio_values)` once; `inputs_embeds = concat(audio_features, inputs_embeds, axis=1)`; create 48 empty KV tensors `[1, 16, 0, 64]` (float16 when the graph input type is float16); `attention_mask = ones[1, L]`; `position_ids = arange(L)`.
   - Run `language_model`; take `logits[:, -1, :]`.
   - Repetition penalty over every id in `generate_tokens` (including the start token): score < 0 -> score * 1.2, else score / 1.2.
   - `next = argmax`; append to `generate_tokens`; stop if `next == 6562`.
   - Append 1 to `attention_mask`; `position_ids = last + 1`; feed `present.N.key/value` back as `past_key_values.N.key/value` (match by index name); `input_ids = [[next]]` (a single id passes through the speech branch of `embed_tokens`).
5. `speech_tokens = concat(audio_tokens, generate_tokens[:, 1:-1], [4299, 4299, 4299])`.
6. `waveform = conditional_decoder(speech_tokens, speaker_embeddings, speaker_features)`; squeeze; write at 24 kHz. The README does not trim the prompt region, so the decoder output already excludes it (inferred).

Parity notes against the PyTorch path (code-read `tts_turbo.py`, `t3.py`):

| Step | PyTorch (`chatterbox-tts` 0.1.7) | ONNX README |
|---|---|---|
| Text normalisation | `punc_norm` (phase 1 doc 1.3) | none |
| Reference loudness | -27 LUFS (pyloudnorm) | none |
| Reference min length | asserts > 5 s | none |
| Speech start | one start token appended after text | two 50256 positions mapped to 6561 |
| Decoding | Temperature 0.8 -> TopK 1000 -> TopP 0.95 -> RepetitionPenalty 1.2 -> multinomial | RepetitionPenalty 1.2 -> argmax |
| Max new tokens | 1000 | 1024 |
| Token cleanup | drop ids >= 6561 | slice `[1:-1]` (drops the last id even if it is not 6562) |
| Watermark | always | off by default |

To match phase 1, port `punc_norm`, -27 LUFS loudness (`ebur128` crate) and the sampler, and drop ids >= 6561 instead of slicing.

## 4. Tokenizer (Rust `tokenizers` crate)

`tokenizer.json`: version "1.0"; model BPE (50257 vocab, 50000 merges, no unk); pre-tokenizer ByteLevel (`add_prefix_space: false`, `use_regex: true`); no normalizer; decoder ByteLevel; post-processor TemplateProcessing `single = [A, <|endoftext|>, <|endoftext|>]`; added special tokens 50256 `<|endoftext|>` and the 19 tags 50257-50275 (same ids as phase 1 doc 1.7).

Tested with `tokenizers = { version = "=0.23.2", default-features = false, features = ["fancy-regex"] }`, `Tokenizer::from_file`, `encode(text, true)` on Windows (Rust 1.99, built in 44 s):

| Input | Ids |
|---|---|
| `Hello world` | `[15496, 995, 50256, 50256]` (15496/995 are GPT-2's ids; checked against `vocab`) |
| `Oh, that's hilarious! [chuckle] Um anyway, how are you doing today?` | `[5812, 11, 326, 338, 20105, 0, 220, 50274, 21039, 6949, 11, 703, 389, 345, 1804, 1909, 30, 50256, 50256]` |
| `[laugh] [clear throat] test` | `[50275, 220, 50267, 1332, 50256, 50256]` |

`encode(text, false)` returns the same ids without the two 50256. Cast `u32` ids to `i64`.

Crate facts (crates.io API): stable `tokenizers` 0.23.2 (2026-09-03, Apache-2.0); 1.0.0-rc.2 is a pre-release. Default features `progressbar`, `onig`, `esaxx_fast`; `onig` and `esaxx_fast` compile C/C++ code with the MSVC toolchain. Regex backend gating (code-read `src/utils/mod.rs` at tag v0.23.2): `onig` wins if enabled; otherwise `fancy-regex`; neither gives `compile_error!`. `fancy-regex` is pure Rust and was tested above. `http` (download from the Hub) is off by default.

## 5. Precision variants: sizes and sha256

Per-variant totals (all four graphs, `.onnx` + `.onnx_data`): fp32 3319.1 MB, fp16 1661.1 MB, q4 721.4 MB, q4f16 561.8 MB, quantized (q8) 1120.2 MB. Mixing is allowed (Resemble's own JS demo for the non-Turbo model mixes fp32 encoders with a q4/q4f16 LM; https://github.com/resemble-ai/transformersjs-chatterbox-demo `src/workers/tts.worker.js`).

| File | .onnx bytes | .onnx sha256 | .onnx_data bytes | .onnx_data sha256 |
|---|---:|---|---:|---|
| `speech_encoder.onnx` | 1172072 | `4d66128037517dd51d370edc9b89ce36d42c75dcbd96e7216c7fb45dfae36045` | 1044712832 | `c9915ff6c529e7bb80983b525255e6744d6c39c7e35b12720925ba99ed0d0a2f` |
| `speech_encoder_fp16.onnx` | 1189888 | `5544f87c3e1615a31f5dc3ccdf9ba52a9f5fb8f6018e72796235fc1cfd5ebc9f` | 522307136 | `d24b817ff61489373ab13033cbdc28a5ba8347341248a00f9a63f10888e81d73` |
| `speech_encoder_q4.onnx` | 1200346 | `37956c20b67bed85a0da4bc83509d67b5969a1b257d1c546516a5236a17ad71e` | 229560112 | `58956db217c6443e49c91bdd54d7cf76b4a243f225c748b7bf746459fc27bc7d` |
| `speech_encoder_q4f16.onnx` | 1217655 | `004407d26127c1d8e974bd213ecff8dbe67611527ca7a1930e656b80d9c51125` | 177289008 | `c017ab57887418e0609afdef52b0cbc17f43cb304e78af44300c0292cd2b2fa9` |
| `speech_encoder_quantized.onnx` | 1205728 | `5b6f15870a43cf97892df86fc550a0ef4763522d527cde72b2a4316f80a34de4` | 354676576 | `d59861fb55e806fbeee731da9d4f8ff819fb5735de5d15e262d902594ee4dbb6` |
| `embed_tokens.onnx` | 2058 | `27796e8252f36b463b0421cafdcc35b5f1e670ab0d96c9182f37ac6571c2f4bc` | 232812544 | `a1c37edc6ec6adb655351f02e958da297221b50211c2c01b69312cb6f008a293` |
| `embed_tokens_fp16.onnx` | 1754 | `f7a7a83e91337e10add2fad054544421f9080e27b72af1aef27f339a1a66776c` | 116406272 | `cdda886a9e58ad39059fe8f6cf5a3a628994a1a2dfc1a8b58502c252fec70327` |
| `embed_tokens_q4.onnx` | 2844 | `fd6ba1d22902e8f539d3dd6d7c1c44b98ebb4c84ebbb5e47fcb826ddcf667561` | 37286384 | `f54a51e234b509b64c3a03bb79e1149fba7e2eba6c2d9c222f18883379e1f5d8` |
| `embed_tokens_q4f16.onnx` | 2548 | `e5c9507c89e770e35d68bdd669e4ae44b34272232e84f010f2c97959feae79b3` | 33648688 | `37e96447b5f3ef46f46fb35cc599cfcb95b277e5528cc0c89bab78169da620ef` |
| `embed_tokens_quantized.onnx` | 2887 | `0efe1bc01c2c48a98425a74444fd9887924d887f922c2722a6ec961ebb9e1db6` | 67297376 | `9025d04c124899823124b1d7bb7069b1f535fb8a6c2d88f97520eb6fecced986` |
| `language_model.onnx` | 207266 | `c12e31df78c74f9589b165c8d51e65171f5028b77b7fedb41900f55f7f410dc8` | 1269724812 | `67db106868f5354b2e425651f1791aef36ae3e6f00ac5e1d91e32c985cad6b39` |
| `language_model_fp16.onnx` | 209456 | `396b35570dd1dbc5f537ffc932c3e69130ab8a09bf1390469d92338c8eae7da4` | 634862406 | `d730d6437126d3e232747b26e2a7adece68844f22133277ca5012298ffe36f93` |
| `language_model_q4.onnx` | 274572 | `b39d03d3f8b943b9e60c6fce3fb41191dbc1df4589f913291db1e214eef669b1` | 204456572 | `2c029dc0acf48752473d8c74c72b5ceaaad76b9886fe106eaf2022142d5b5d5e` |
| `language_model_q4f16.onnx` | 276803 | `4c5d83674c52bb71683a1ea1f60e2608b80cda48aafce67e8f8c03162c474ad5` | 183981430 | `f084f21f977e365e76e622d2958c309ff894537f7b7133ac7fc5ce8fc055d6f9` |
| `language_model_quantized.onnx` | 279670 | `0b40581277e30b7034331ec8c3ad47ed71d321f015b387a95221e54e2fcbfde8` | 367962860 | `ec9945df36cb5d131d46688f2609fd715fbfcb0b8ee9681af5c84118de2d55a2` |
| `conditional_decoder.onnx` | 1889468 | `8c43f3a1d0ddb1a86e226a244d7cda5396c67f5c6412789c23900c646e3ffc50` | 768593792 | `05f162a519f3e9abaf0b7337ae037f4af8b2b30c4455d39b2c61ed3a9b2b5476` |
| `conditional_decoder_fp16.onnx` | 2104016 | `cbdc0281548eb90a02fa2430647237df63fa0cb695e42b91c10b860b9b6d2230` | 384019328 | `c6c3e79e6ff86bc41f77381a3d67b8edaa16b13e43a2700ca8159d0894bd594a` |
| `conditional_decoder_q4.onnx` | 2179022 | `dccb7a6cea3472dc7f7d070eeb70ade18e6327fb4ec61a3d62cf211bfed90ea2` | 246397384 | `b5c5317e0b79a1a19dd3d5e2b2091ea06b15716716ab801a54eaeb906c6971ec` |
| `conditional_decoder_q4f16.onnx` | 2394210 | `17c9f7caef818605df2dc197c2f41682c0bda7fe437b8652dae8366de8b68b7e` | 162996136 | `3e53cc7cc05e72957797c620e1bf0c6d6b909df937de4756c6e534d50854ca3e` |
| `conditional_decoder_quantized.onnx` | 2202035 | `2af3b150196d9d559cd3c91e03da80eb27a466032369dc2b57ea729cddad3ebb` | 326548688 | `4918ca09e05e41d2b4aa1ace6201d1cd911ffc58a42801002bab177d495cfe0a` |

Rust-side verification: stream each download through `sha2::Sha256` and compare with this table (these are the HF API `lfs.sha256` values; the `X-Linked-ETag` response header carries the same value for LFS files). Write to a temp name and rename only after the hash matches.

## 6. Reference audio and output

- Input: the same clips `narrate setup` creates (24 kHz WAV). Read with `hound`; resample other rates to 24 kHz (`rubato` 5.0.1, MSRV 1.87); downmix to mono.
- Output: `waveform` float32 at 24 kHz; write with `hound`.

## 7. ONNX Runtime from Rust (`ort`)

### 7.1 Crate facts (rc.13; crates.io API, GitHub tag `v2.0.0-rc.13` = commit `002f41a8e175eac7f6695ff361d2e51a50874c48`)

- Versions: all 1.x releases are yanked; 2.0.0-rc.0 to rc.13 are pre-releases. rc.13: edition 2024, `rust-version = "1.88"`, license MIT OR Apache-2.0. Pin with `=2.0.0-rc.13`.
- Default features: `std`, `ndarray`, `tracing`, `download-binaries`, `tls-native`, `copy-dylibs`, `api-27` (Cargo.toml). The docs page `setup/multiversion.mdx` says `api-28` is default; Cargo.toml is authoritative.
- ONNX Runtime version: "v2.0.0+ | v1.28.0" (`misc/version-mapping.mdx`); supports runtimes 1.17 to 1.28 through `api-17` ... `api-28`. `ORT_API_VERSION = 17 + enabled api features` (`ort-sys/src/version.rs`).
- Dependencies: `ndarray 0.17`, optional `half 2.1`, `libloading 0.9` (load-dynamic). Enable `half` to build the empty float16 KV inputs for fp16/q4f16 language models (docs: "Enables support for creating & extracting float16/bfloat16 tensors via the half crate").
- EP registration API (docs `perf/execution-providers.mdx`): `Session::builder()?.with_execution_providers([ep::CUDA::default().build().error_on_failure()])?.commit_from_file(path)?`. Without `error_on_failure()` ort "will silently fail and fall back to executing on the CPU".

### 7.2 Linking options

| Option | Source of `onnxruntime.dll` | Notes |
|---|---|---|
| `download-binaries` (default) | `cdn.pyke.io` (pyke.io builds of ORT 1.28.0, statically linked, x86-64-v3 baseline, small patches) | Windows feature sets in `dist.tsv`: `directml`, `webgpu`, `nvrtx,directml`, `cuda13,tensorrt,nvrtx,directml`. Enabling `cuda` resolves to `cuda13` by default (no CUDA 12 build is shipped: "we only ship 13 for now", `resolve.rs`). CUDA build URL `https://cdn.pyke.io/0/pyke:ort-rs/ms@1.28.0/x86_64-pc-windows-msvc+cuda13,tensorrt,nvrtx,directml.tar.lzma2`, sha256 `efb686cb49318e476cbb65d8ef40d4005f541a40bdee66afb7b60e3714e78293`. Docs: "binaries for CUDA >= 13.2 and targets cuDNN >= 9.23", which must be on `PATH`. Builds are SLSA-attested since rc.12 (`gh attestation verify --owner pykeio`). Not a Microsoft artifact. |
| `load-dynamic` | any `onnxruntime.dll` you choose, loaded at run time | `ort::init_from(path)?.commit()` "must be called before any other usage of ort"; or `ORT_DYLIB_PATH` (relative paths resolve against the executable's folder). Version check: minor version below the API level -> `LoadError::BadVersion`; above -> info log only. |
| Static or compile-time dynamic (`ORT_LIB_PATH`, `ORT_PREFER_DYNAMIC_LINK=1`) | your own build | not needed |

Recommended for official sources: `ort = { version = "=2.0.0-rc.13", default-features = false, features = ["std", "ndarray", "half", "load-dynamic", "cuda", "api-27"] }` (add `"directml"` only if the DML path in 7.5 is used) and Microsoft's ORT 1.28.3 CUDA zip.

### 7.3 Official Microsoft artifacts (GitHub release assets with digests, https://github.com/microsoft/onnxruntime/releases)

| Asset | Bytes | sha256 |
|---|---:|---|
| `v1.28.3/onnxruntime-win-x64-gpu_cuda13-1.28.3.zip` (2026-10-02) | 366180839 | `a7216e498c14fb4783a42667ffddcf1590ae262429307ed0dc5889d9c365584e` |
| `v1.28.3/onnxruntime-win-x64-gpu_cuda12-1.28.3.zip` | 454803675 | `65d7f956649df80a8ccdcafa7843e4b02f38bfc9e6de28711ed8384508040330` |
| `v1.28.3/onnxruntime-win-x64-1.28.3.zip` (CPU) | 78606946 | `1d6fab48e85f948436af7c8c971d2c145cf224e2c444755dec894f8b0de11a83` |
| `v1.31.0/onnxruntime-win-x64-gpu_cuda13-1.31.0.zip` (latest, 2026-10-09) | 309484217 | `d332ccf84acb769a11364cddd4e8d2992a168d16be472d5ba70b279b644552f5` |

- The cuda12 1.28.3 zip contains `lib/onnxruntime.dll`, `onnxruntime_providers_shared.dll`, `onnxruntime_providers_cuda.dll`, `onnxruntime_providers_tensorrt.dll` plus `.lib`/`.pdb`; no CUDA or cuDNN DLLs (tested listing). Ship only the three DLLs needed (skip `.pdb`, about 0.9 GB).
- NuGet `Microsoft.ML.OnnxRuntime.Gpu.Windows` latest 1.31.0 (CUDA 13 since 1.27 per the ORT doc table); 1.28.3 is on GitHub releases only.
- 1.28.x matches ort rc.13's API exactly; 1.29-1.31 also load (newer minor, info log).

### 7.4 CUDA requirements for the official zip

ORT doc table: 1.27.x-1.30.x CUDA-13 packages need CUDA 13.0 and cuDNN 9.x; CUDA-12 packages need "CUDA 12.8 or newer" and cuDNN 9.x; "ONNX Runtime built with cuDNN 8.x is not compatible with cuDNN 9.x, and vice versa". Driver 610.88 is branch R610, and NVIDIA lists CUDA 13.3 as R610 and the CUDA 13.x minor-version-compatibility floor as ">= 580" (https://docs.nvidia.com/cuda/cuda-toolkit-release-notes/index.html).

Libraries the CUDA provider uses (code-read `cmake/onnxruntime_providers_cuda.cmake` lines 403-408 at v1.28.3): links cublasLt, cublas, curand, cudart, CUDA driver; "cuDNN and cuFFT are loaded dynamically at runtime".

Official NVIDIA redistributables on PyPI that match the `onnxruntime-gpu` 1.31.0 extras (`nvidia-cuda-runtime~=13.0`, `nvidia-cuda-nvrtc~=13.0`, `nvidia-cufft~=12.0`, `nvidia-curand~=10.0`, `nvidia-cudnn-cu13~=9.0`, which pulls `nvidia-cublas`):

| Package | Latest with a `win_amd64` wheel | Wheel bytes |
|---|---|---:|
| `nvidia-cuda-runtime` | 13.4.92 | 2778543 |
| `nvidia-cublas` | 13.8.0.4 (13.8.1.7 has no Windows wheel) | 423266897 |
| `nvidia-cufft` | 12.4.0.43 (depends on `nvidia-nvjitlink`) | 160953139 |
| `nvidia-curand` | 10.4.4.72 | 56731059 |
| `nvidia-cuda-nvrtc` | 13.4.92 | 46972283 |
| `nvidia-cudnn-cu13` | 9.27.0.42 | 436469905 |

These can be installed into a separate uv project under the data dir and their `bin` folders added to the DLL search path (prepend to `PATH` for the process, or `AddDllDirectory`) before `ort::init_from`. Exact DLL file names inside these wheels are unverified. The NVIDIA CUDA Toolkit 13.x and cuDNN 9 installers from developer.nvidia.com are the other official source.

### 7.5 EP kernel coverage for the ops these graphs use (ORT v1.28.3 `docs/OperatorKernels.md`)

| Op | CPU | CUDA | DirectML |
|---|---|---|---|
| `GroupQueryAttention` | float, float16 | float16, bfloat16 only | float, float16 |
| `MultiHeadAttention` | float | float, float16, bfloat16 | float, float16 |
| `MatMulNBits` | float, float16 | float, float16, bfloat16 | float, float16 |
| `GatherBlockQuantized` | yes | yes | no |
| `STFT` | yes | no | yes |
| `RandomNormalLike`, `RandomUniformLike` | yes | yes | no |

Unsupported nodes run on the CPU EP, with device copies. Expected CUDA placement (inferred): STFT in speech_encoder and conditional_decoder on CPU; GQA on CPU unless the fp16/q4f16 LM is used.

DirectML: ORT docs say "DirectML is in sustained engineering" and "new feature development has moved to WinML"; requires disabling memory pattern and parallel execution; only one `Run` at a time per session. Microsoft's `Microsoft.ML.OnnxRuntime.DirectML` NuGet stops at 1.24.4, so using it with ort needs `api-24` instead of `api-27`. System `directml.dll` here is 1.15.5; ORT's DML EP targets DirectML 1.15.2. pyke's Windows builds all include DML.

### 7.6 Other runtime notes

- VRAM: fp16 weights total 1.66 GB versus 3.32 GB for fp32 (estimate of resident weight size; activations and KV cache extra). See phase 1 doc section 9 for measurement.
- KV cache traffic: feeding `present.*` back as host tensors copies 48 tensors per step; ort's IoBinding (`docs perf/io-binding.mdx`) can keep them on the GPU. Performance impact unmeasured.
- ort logs through `tracing` when that feature is on; otherwise ORT logs to stderr.

## 8. Python ONNX baseline (optional, for debugging)

`onnxruntime-gpu` 1.31.0 has a cp311 `win_amd64` wheel and `requires_python >=3.11`; extras `[cuda,cudnn]` pull the NVIDIA wheels above. Not needed by `narrate-onnx`.

## 9. Official PerTh watermarker port: none found

| Place checked | Result |
|---|---|
| GitHub org `resemble-ai` (56 public repos, API listing) | `Perth` is Python only: its tree has `.py` files and `perth_net_250000.pth.tar`, no `.onnx` and no Rust. `transformersjs-chatterbox-demo` (commit `4608b286`): its worker, hooks and audio/client libs (6 files checked) have no `watermark` or `perth` references. |
| Hugging Face author `ResembleAI` (20 models) | No PerTh model. |
| PyPI `resemble-perth` | Versions 1.0.0 and 1.0.1, Python wheel with a PyTorch checkpoint. |
| crates.io search "perth" | No related crate. |
| Third party | `onnx-community/perth-watermarker-ONNX` (HF org `onnx-community`, commit `2997df2497116bfd2f964718ca7becf6e289d74a`, 2025-10-17): `onnx/implicit_watermarker_{16000,24000,44100,48000}.onnx` + `.onnx_data`. Not published by Resemble AI, so it does not meet the user's rule. |
| Official ONNX README | `apply_watermark = False` by default; when enabled it calls the Python `perth.PerthImplicitWatermarker()`. |

Answer: no official ONNX export or Rust port exists as of 2026-10-12.

## Unverified

- That the fp16/q4f16 graphs run fully on the CUDA EP apart from STFT (needs ORT verbose logs on this machine).
- Exact DLL names in the NVIDIA PyPI wheels and whether ORT 1.28.3 loads them from `PATH` without extra configuration.
- Output quality and speed of each precision variant.
- PyTorch tokenizer behavior under transformers 5.2.0 for the turbo repo's `vocab.json`/`merges.txt` (whether it also appends end-of-text tokens).
- Whether `conditional_decoder` excludes the prompt region (inferred from the README not trimming it).
