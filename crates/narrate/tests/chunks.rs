//! Chunked generation with the fake worker: verbose sample sums, pauses, request order and
//! seeds, failure hooks, worker session protocol errors, worker environment and logs
//! (FR-005, FR-002 pauses, FR-012, NFR-002, NFR-012).
//!
//! Sources: specs/spec-narrate-generation.md (Requirements 7, 10, 14; RULE-001 to RULE-007),
//! specs/spec-narrate-cli.md (line catalog), specs/spec-narrate-setup.md (Child process
//! environment), specs/spec-narrate-worker.md (Requirement 9). No implementation was read.

mod common;

use common::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn paragraphs(n: usize) -> String {
    (1..=n).map(|k| format!("Paragraph number {k} is here.")).collect::<Vec<_>>().join("\n\n")
}

fn prose(seed: usize, sentences: usize) -> String {
    let words = [
        "amber", "river", "quiet", "lantern", "morning", "bridge", "violet", "harvest", "signal",
        "meadow", "copper", "window",
    ];
    let mut out = Vec::new();
    for sent_no in 0..sentences {
        let count = 7 + (seed + sent_no) % 4;
        let mut sentence = String::new();
        for w in 0..count {
            if w > 0 {
                sentence.push(' ');
            }
            sentence.push_str(words[(seed * 5 + sent_no * 3 + w) % words.len()]);
        }
        sentence.push('.');
        out.push(sentence);
    }
    out.join(" ")
}

fn script_file(dir: &Path, name: &str, text: &str) -> PathBuf {
    write_text(dir, name, text)
}

// ---------------------------------------------------------------------------------------
// Long script output: sample sums and pauses
// ---------------------------------------------------------------------------------------

// [Scenario: Long script output | spec-narrate-generation Req 10 | FR-005 | FR-002 | UR-001]
#[test]
fn verbose_sample_counts_sum_to_the_wav_and_pauses_follow_the_rules() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    let md = format!(
        "# Chapter Title\n\n{}\n\n{}\n\n{}\n",
        prose(1, 9),
        prose(2, 9),
        prose(3, 9)
    );
    assert!(md.chars().count() >= 1400, "fixture is about 1,500 characters, got {}", md.chars().count());
    let script = script_file(work.path(), "long.md", &md);
    let out = ready.nar().log(&log).arg(&script).arg("--verbose").run();
    out.assert_code(0);

    let wavs = wav_or_tmp_files(work.path());
    assert_eq!(wavs, vec!["long.wav".to_string()], "exactly one WAV and no temporary file");
    let wav_samples_total = assert_output_wav_format(&work.path().join("long.wav")) as u64;

    let v = parse_verbose(&out);
    let n = v.chunk_samples.len();
    assert!(n >= 7, "heading plus three multi-chunk paragraphs, got {n} chunks");
    assert_eq!(v.silences.len(), n, "a silence follows every chunk, the last one included");
    assert_eq!(v.texts.len(), n);
    assert!(out.has_stderr_line(&format!("verbose: chunks {n}")), "{}", out.context());

    let sum: u64 = v.chunk_samples.iter().map(|(_, _, s)| *s).sum::<u64>() + v.silences.iter().map(|(_, s)| *s).sum::<u64>();
    assert_eq!(sum, wav_samples_total, "WAV sample count equals the sum of all printed samples= values");

    // Pauses: heading 19200 once (after chunk 1), paragraph 12000 three times (after the last
    // chunk of each paragraph, the final chunk of the script included), chunk 4800 elsewhere.
    assert_eq!(v.silences.iter().filter(|(_, s)| *s == 19_200).count(), 1);
    assert_eq!(v.silences[0], (1usize, 19_200u64), "the heading chunk comes first and takes the heading pause");
    assert_eq!(v.silences.iter().filter(|(_, s)| *s == 12_000).count(), 3);
    assert_eq!(v.silences.last().unwrap().1, 12_000, "the run's last chunk is followed by its pause");
    assert_eq!(v.silences.iter().filter(|(_, s)| *s == 4_800).count(), n - 4);
    assert!(v.silences.iter().all(|(_, s)| [4_800u64, 7_200, 12_000, 19_200].contains(s)));

    // The printed chunk text is the text sent, and the fake returns 100 samples per character.
    let sent = synth_texts(&log);
    assert_eq!(sent.len(), n);
    for (i, (idx, total, text)) in v.texts.iter().enumerate() {
        assert_eq!((*idx, *total), (i + 1, n));
        assert_eq!(text, &sent[i]);
        assert!(text.chars().count() <= 300);
        assert_eq!(v.chunk_samples[i], (i + 1, n, text.chars().count() as u64 * 100));
    }
    assert_eq!(sent[0], "Chapter Title");

    // `chunk i/N done` progress lines, in order.
    for i in 1..=n {
        assert!(out.has_stderr_line(&format!("chunk {i}/{n} done")), "missing progress line {i}");
    }
}

// [Scenario: Long script output, table pauses | spec-narrate-generation Req 10 | FR-002 | RULE-021]
#[test]
fn table_cells_use_the_cell_pause_in_generated_audio() {
    let ready = Ready::new();
    let work = scratch();
    let script = script_file(work.path(), "table.md", "| Name | Age |\n|------|-----|\n| Ann  | 30  |\n");
    let out = ready.nar().arg(&script).arg("--verbose").run();
    out.assert_code(0);
    let v = parse_verbose(&out);
    assert_eq!(v.silences.iter().map(|(_, s)| *s).collect::<Vec<_>>(), vec![7_200, 12_000, 7_200, 12_000]);
}

// [Scenario: Seeds and order | spec-narrate-generation Req 10 | FR-005 | RULE-002]
#[test]
fn synth_requests_are_in_order_with_total_and_seed_equal_to_chunk() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    let script = script_file(work.path(), "four.txt", &paragraphs(4));
    ready.nar().log(&log).arg(&script).run().assert_code(0);
    let synth = requests_of(&log, "synth");
    assert_eq!(synth.len(), 4);
    for (i, e) in synth.iter().enumerate() {
        let r = &e["request"];
        assert_eq!(r["chunk"].as_u64(), Some(i as u64 + 1));
        assert_eq!(r["total"].as_u64(), Some(4));
        assert_eq!(r["seed"].as_u64(), Some(i as u64 + 1), "seed equals the chunk number");
        assert_eq!(r["voice_id"], "female");
        assert_eq!(r["text"].as_str(), Some(format!("Paragraph number {} is here.", i + 1).as_str()));
    }
}

// [Spec-narrate-generation Data flow, Req 7 | FR-005]
#[test]
fn session_follows_hello_load_voice_synth_shutdown_with_sequential_ids() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    let script = script_file(work.path(), "two.txt", &paragraphs(2));
    ready.nar().log(&log).arg(&script).run().assert_code(0);
    assert_eq!(op_sequence(&log), vec!["hello", "load", "voice", "synth", "synth", "shutdown"]);
    for (i, e) in log_requests(&log).iter().enumerate() {
        assert_eq!(e["request"]["id"].as_u64(), Some(i as u64 + 1), "ids are 1, 2, 3, ... per session");
    }
    let hello = &requests_of(&log, "hello")[0];
    assert_eq!(hello["request"]["protocol"].as_u64(), Some(1));
    assert_eq!(worker_pids(&log).len(), 1, "one worker per run");
}

// ---------------------------------------------------------------------------------------
// Failures
// ---------------------------------------------------------------------------------------

// [Scenario: One chunk fails | spec-narrate-generation Req 10 / spec-narrate-cli Hook active | FR-005 | NFR-007]
#[test]
fn chunk_failure_hook_stops_the_run_names_the_chunk_and_never_retries() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    let script = script_file(work.path(), "five.txt", &paragraphs(5));
    let out = ready.nar().log(&log).env("NARRATE_TEST_FAIL_CHUNK", "3").arg(&script).run();
    out.assert_error_prefix(1, "chunk-failed", "generation failed on chunk 3 of 5: ", "rerun with --verbose for details");
    out.assert_stderr_contains("chunk 3 of 5");
    assert!(wav_or_tmp_files(work.path()).is_empty(), "no .wav and no *.narrate-*.tmp file");
    let chunks_requested: Vec<u64> = requests_of(&log, "synth").iter().map(|e| e["request"]["chunk"].as_u64().unwrap()).collect();
    assert_eq!(chunks_requested, vec![1, 2, 3], "chunk 3 is requested once (no retry) and chunks 4 and 5 never");
}

// [Mapping of worker codes | spec-narrate-worker | FR-005 | NFR-007]
#[test]
fn synth_failed_and_out_of_memory_replies_map_to_their_catalog_codes() {
    let ready = Ready::new();
    let work = scratch();
    let script = script_file(work.path(), "one.txt", "Hello there.");
    let out = ready.nar().env("FAKE_WORKER_FAIL_OP", "synth:synth_failed").arg(&script).run();
    out.assert_error_prefix(1, "chunk-failed", "generation failed on chunk 1 of 1: ", "rerun with --verbose for details");

    let out = ready.nar().env("FAKE_WORKER_FAIL_OP", "synth:out_of_memory").arg(&script).run();
    out.assert_error(
        1,
        "gpu-out-of-memory",
        "the GPU ran out of memory on chunk 1 of 1",
        "close other programs that use the GPU, or use --device cpu",
    );
    assert!(wav_or_tmp_files(work.path()).is_empty());
}

// [Scenario: Worker crashes on a chunk | spec-narrate-generation Req 10 | FR-005 | RULE-007]
#[test]
fn worker_crash_reports_worker_exited_with_chunk_and_exit_code_and_keeps_its_log() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    let script = script_file(work.path(), "three.txt", &paragraphs(3));
    let out = ready.nar().log(&log).env("NARRATE_TEST_CRASH_CHUNK", "2").arg(&script).run();
    out.assert_code(1);
    assert!(
        out.has_stderr_line("error[worker-exited]: the generation worker exited unexpectedly (exit code 3) during chunk 2 of 3"),
        "{}",
        out.context()
    );
    out.assert_stderr_contains("chunk 2 of 3");
    assert!(wav_or_tmp_files(work.path()).is_empty());

    // The failed run keeps `logs\worker-<pid>.log`, and the fix line points at it.
    let pids = worker_pids(&log);
    assert_eq!(pids.len(), 1);
    let log_path = ready.d().join("logs").join(format!("worker-{}.log", pids[0]));
    assert!(log_path.is_file(), "worker log kept after a failure: {}", log_path.display());
    assert!(out.has_stderr_line(&format!("  fix: rerun with --verbose, or read {}", log_path.display())), "{}", out.context());
    assert!(read_lossy(&log_path).contains("started pid"), "the worker's stderr is captured in its log");
}

// [Error catalog: worker-exited outside synth | spec-narrate-cli | FR-005 | NFR-007]
#[test]
fn worker_exit_outside_synth_names_the_operation_instead_of_a_chunk() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    let script = script_file(work.path(), "one.txt", "Hello there.");
    // `FAKE_WORKER_EXIT_OP=<op>`: the fake exits with code 3 without replying to that op.
    for op in ["load", "voice"] {
        let out = ready.nar().log(&log).env("FAKE_WORKER_EXIT_OP", op).arg(&script).run();
        out.assert_code(1);
        assert!(
            out.has_stderr_line(&format!(
                "error[worker-exited]: the generation worker exited unexpectedly (exit code 3) while {op}"
            )),
            "{op}\n{}",
            out.context()
        );
        assert!(!out.stderr.contains("during chunk"), "{op}\n{}", out.context());
        assert!(wav_or_tmp_files(work.path()).is_empty());
        let _ = fs::remove_file(&log);
    }
}

// [Business rule RULE-007 | spec-narrate-generation | NFR-007]
#[test]
fn worker_log_is_deleted_after_success_and_old_logs_are_purged() {
    let ready = Ready::new();
    let work = scratch();
    let script = script_file(work.path(), "one.txt", "Hello there.");
    let logs = ready.path().join("logs");
    fs::create_dir_all(&logs).unwrap();
    let old = write_text(&logs, "worker-111.log", "old log");
    let fresh = write_text(&logs, "worker-222.log", "fresh log");
    let other = write_text(&logs, "notes.txt", "not a worker log");
    age_file(&old, 8 * 24);
    age_file(&other, 8 * 24);
    ready.nar().arg(&script).run().assert_code(0);
    assert!(!old.exists(), "worker logs older than 7 days are deleted at the start of a run");
    assert!(fresh.exists(), "recent worker logs stay");
    assert!(other.exists(), "files narrate does not own are never deleted");
    let worker_logs: Vec<String> = list_dir(&logs).into_iter().filter(|n| n.starts_with("worker-") && n.ends_with(".log")).collect();
    assert_eq!(worker_logs, vec!["worker-222.log".to_string()], "this run's own log is deleted after success");
}

// ---------------------------------------------------------------------------------------
// Model limit warning
// ---------------------------------------------------------------------------------------

// [Scenario: Chunk at the model limit | spec-narrate-generation Req 10 | RULE-005 | FR-005]
#[test]
fn chunk_at_or_above_948000_samples_warns_and_the_run_still_succeeds() {
    let ready = Ready::new();
    let work = scratch();
    let script = script_file(work.path(), "limit.txt", "Hello there.");
    let warning = "warning: chunk 1/1 reached the 40 s model limit; its speech may be cut off";
    for samples in ["960000", "948000"] {
        let out = ready.nar().env("FAKE_WORKER_SYNTH_SAMPLES", samples).arg(&script).run();
        out.assert_code(0);
        assert!(out.has_stderr_line(warning), "{samples}\n{}", out.context());
        out.assert_stderr_contains("warning: chunk 1/");
        out.assert_stderr_contains("reached the 40 s model limit");
    }
    let out = ready.nar().env("FAKE_WORKER_SYNTH_SAMPLES", "947999").arg(&script).run();
    out.assert_code(0);
    out.assert_stderr_lacks("reached the 40 s model limit");
}

// ---------------------------------------------------------------------------------------
// Worker session errors
// ---------------------------------------------------------------------------------------

// [Scenario: Worker cannot start | spec-narrate-generation Req 7 | FR-005 | NFR-007]
#[test]
fn nonexistent_worker_executable_exits_1_with_worker_start() {
    let ready = Ready::new();
    let work = scratch();
    let script = script_file(work.path(), "one.txt", "Hello there.");
    let out = Nar::new(ready.path()).env("NARRATE_WORKER_CMD", r"C:\does-not-exist\narrate-no-worker.exe").arg(&script).run();
    out.assert_error_prefix(
        1,
        "worker-start",
        "could not start the generation worker: ",
        "run narrate setup to repair the environment",
    );
    assert!(wav_or_tmp_files(work.path()).is_empty());
}

// [Scenario: Protocol mismatch | spec-narrate-generation Req 7 | FR-005 | NFR-007]
#[test]
fn hello_with_protocol_2_is_a_worker_protocol_error() {
    let ready = Ready::new();
    let work = scratch();
    let script = script_file(work.path(), "one.txt", "Hello there.");
    let out = ready.nar().env("FAKE_WORKER_PROTOCOL", "2").arg(&script).run();
    out.assert_error_prefix(
        1,
        "worker-protocol",
        "the generation worker sent an invalid message: ",
        "run narrate setup to repair the environment",
    );
    assert!(wav_or_tmp_files(work.path()).is_empty());
}

// [Scenario: Host rejects a mismatched id | spec-narrate-worker Req 3 | FR-005]
#[test]
fn reply_with_the_wrong_id_stops_the_run_and_terminates_the_worker() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    let script = script_file(work.path(), "one.txt", "Hello there.");
    let out = ready.nar().log(&log).env("FAKE_WORKER_WRONG_ID_OP", "synth").arg(&script).run();
    out.assert_error_prefix(
        1,
        "worker-protocol",
        "the generation worker sent an invalid message: ",
        "run narrate setup to repair the environment",
    );
    assert!(wav_or_tmp_files(work.path()).is_empty());
    let pid = worker_pids(&log)[0];
    assert!(wait_until(Duration::from_secs(10), || !pid_alive(pid)), "the worker must be terminated");
}

// [Error catalog: worker-protocol (non-JSON line) | spec-narrate-cli | FR-005]
#[test]
fn non_json_reply_is_a_worker_protocol_error_for_every_session_step() {
    let ready = Ready::new();
    let work = scratch();
    let script = script_file(work.path(), "one.txt", "Hello there.");
    for op in ["hello", "load", "voice", "synth"] {
        let out = ready.nar().env("FAKE_WORKER_GARBAGE_OP", op).arg(&script).run();
        out.assert_code(1);
        out.assert_stderr_contains("error[worker-protocol]");
        assert!(wav_or_tmp_files(work.path()).is_empty(), "{op}");
    }
}

// ---------------------------------------------------------------------------------------
// Verbose lines, worker output, done line
// ---------------------------------------------------------------------------------------

// [Scenario: Worker starts + verbose lines | spec-narrate-generation Req 7 / spec-narrate-cli line catalog | FR-005 | FR-024]
#[test]
fn verbose_prints_data_directory_chunk_count_worker_pid_and_model_load() {
    let ready = Ready::new();
    let work = scratch();
    let log = work.path().join("worker.log");
    let script = script_file(work.path(), "two.txt", &paragraphs(2));
    let out = ready.nar().log(&log).arg(&script).arg("--verbose").run();
    out.assert_code(0);
    assert!(out.has_stderr_line(&format!("verbose: data directory {}", ready.d().display())), "{}", out.context());
    assert!(out.has_stderr_line("verbose: chunks 2"), "{}", out.context());
    let pid_line = out.find_stderr_prefix("verbose: worker pid ").expect("worker pid line");
    assert!(pid_line["verbose: worker pid ".len()..].bytes().all(|b| b.is_ascii_digit()), "{pid_line}");
    assert_eq!(verbose_worker_pid(&out), Some(worker_pids(&log)[0]), "the printed pid is the worker's pid");
    let load_line = out.find_stderr_prefix("verbose: model load ").expect("model load line");
    assert!(load_line.ends_with(" s"), "{load_line}");
    assert_eq!(out.stdout.trim_end(), abs(&work.path().join("two.wav")).display().to_string());
}

// [Requirement 7: worker stderr | spec-narrate-generation | FR-005 | FR-024]
#[test]
fn worker_stderr_lines_are_shown_as_worker_lines_only_with_verbose() {
    let ready = Ready::new();
    let work = scratch();
    let script = script_file(work.path(), "one.txt", "Hello there.");
    let quiet = ready.nar().arg(&script).run();
    quiet.assert_code(0);
    assert!(!quiet.stderr.lines().any(|l| l.starts_with("worker: ")), "{}", quiet.context());
    assert!(!quiet.stderr.lines().any(|l| l.starts_with("verbose:")), "{}", quiet.context());

    let loud = ready.nar().arg(&script).arg("--verbose").run();
    loud.assert_code(0);
    assert!(
        loud.stderr.lines().any(|l| l.starts_with("worker: ") && l.contains("started pid")),
        "{}",
        loud.context()
    );
}

// [Output: done line | spec-narrate-generation | NFR-012 | NFR-006]
#[test]
fn done_line_reports_path_audio_samples_synthesis_and_rtf() {
    let ready = Ready::new();
    let work = scratch();
    let script = script_file(work.path(), "rtf.txt", &paragraphs(2));
    let out = ready.nar().arg(&script).run();
    out.assert_code(0);
    let wav = abs(&work.path().join("rtf.wav"));
    let samples = wav_samples(&wav) as f64;

    let line = out.find_stderr_prefix("done: ").unwrap_or_else(|| panic!("{}", out.context()));
    let prefix = format!("done: {} (", wav.display());
    let rest = line.strip_prefix(prefix.as_str()).unwrap_or_else(|| panic!("done line prefix {prefix:?} in {line:?}"));
    let rest = rest.strip_suffix(')').expect("closing parenthesis");
    let parts: Vec<&str> = rest.split(", ").collect();
    assert_eq!(parts.len(), 4, "{line}");
    let audio: f64 = parts[0].strip_suffix(" s audio").expect("audio part").parse().unwrap();
    let n: f64 = parts[1].strip_suffix(" samples").expect("samples part").parse().unwrap();
    let synthesis: f64 = parts[2].strip_prefix("synthesis ").and_then(|p| p.strip_suffix(" s")).expect("synthesis part").parse().unwrap();
    let rtf_text = parts[3].strip_prefix("rtf ").expect("rtf part");
    let rtf: f64 = rtf_text.parse().unwrap();

    assert_eq!(n, samples, "the printed sample count is the WAV's");
    assert!((audio - samples / 24_000.0).abs() <= 0.051, "audio seconds {audio}");
    // The fake reports 0.5 s of synthesis per chunk: 2 chunks.
    assert!((synthesis - 1.0).abs() <= 0.051, "synthesis seconds {synthesis}");
    let (int_part, frac) = rtf_text.split_once('.').expect("rtf has a decimal point");
    assert!(!int_part.is_empty() && frac.len() == 2, "rtf uses two decimals: {rtf_text}");
    let true_rtf = 1.0 / (samples / 24_000.0);
    assert!((rtf - true_rtf).abs() <= 0.011, "rtf {rtf} vs {true_rtf}");

    let done_at = out.stderr.lines().position(|l| l.starts_with("done: ")).unwrap();
    let last_chunk_at = out.stderr.lines().position(|l| l == "chunk 2/2 done").unwrap();
    assert!(last_chunk_at < done_at);
}

// ---------------------------------------------------------------------------------------
// Worker environment (NFR-002, FR-012)
// ---------------------------------------------------------------------------------------

// [Scenario: Worker environment is offline | spec-narrate-generation Req 14 | FR-012 | NFR-002]
#[test]
fn worker_environment_is_offline_redirected_and_free_of_inherited_tool_variables() {
    let ready = Ready::new();
    let work = scratch();
    let script = script_file(work.path(), "one.txt", "Hello there.");
    let dump_template = work.path().join("env-{pid}.json");
    let out = ready
        .nar()
        .env("FAKE_WORKER_ENV_DUMP", &dump_template)
        // Removed prefixes: UV_, HF_, HUGGINGFACE_, TRANSFORMERS_, PIP_, PYTHON, CONDA_,
        // VIRTUAL_ENV, NARRATE_TEST_.
        .env("UV_CACHE_DIR", r"C:\bogus-uv")
        .env("UV_INDEX_URL", "https://evil.example.com/simple")
        .env("HF_ENDPOINT", "https://evil.example.com")
        .env("HUGGINGFACE_HUB_TOKEN", "secret")
        .env("TRANSFORMERS_CACHE", r"C:\bogus-tf")
        .env("PIP_INDEX_URL", "https://evil.example.com/simple")
        .env("PYTHONPATH", r"C:\bogus-py")
        .env("PYTHONHOME", r"C:\bogus-home")
        .env("CONDA_PREFIX", r"C:\conda")
        .env("VIRTUAL_ENV", r"C:\other-venv")
        .env("NARRATE_TEST_BOGUS", "1")
        // Pass-through: proxies, CUDA_VISIBLE_DEVICES, FAKE_WORKER_*, and the worker hooks.
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .env("HTTP_PROXY", "http://127.0.0.1:9")
        .env("CUDA_VISIBLE_DEVICES", "-1")
        .env("FAKE_WORKER_PASSTHROUGH", "yes")
        .env("NARRATE_TEST_FAIL_CHUNK", "99")
        .env("NARRATE_TEST_CRASH_CHUNK", "99")
        .arg(&script)
        .run();
    out.assert_code(0);

    let dumps: Vec<PathBuf> = list_dir(work.path())
        .into_iter()
        .filter(|n| n.starts_with("env-") && n.ends_with(".json"))
        .map(|n| work.path().join(n))
        .collect();
    assert_eq!(dumps.len(), 1, "one worker, one dump: {dumps:?}");
    let dump = read_json(&dumps[0]);

    // Inherited tool variables are gone.
    for gone in [
        "UV_CACHE_DIR", "UV_INDEX_URL", "HF_ENDPOINT", "HUGGINGFACE_HUB_TOKEN", "TRANSFORMERS_CACHE",
        "PIP_INDEX_URL", "PYTHONPATH", "PYTHONHOME", "CONDA_PREFIX", "VIRTUAL_ENV", "NARRATE_TEST_BOGUS",
    ] {
        assert_eq!(env_get(&dump, gone), None, "{gone} must not reach the worker");
    }
    for name in env_names(&dump) {
        let up = name.to_ascii_uppercase();
        assert!(!up.starts_with("UV_"), "inherited UV_ variable {name}");
        assert!(!up.starts_with("PIP_") && !up.starts_with("CONDA_") && !up.starts_with("PYTHON") && !up.starts_with("HUGGINGFACE_"), "{name}");
        if up.starts_with("NARRATE_TEST_") {
            assert!(
                ["NARRATE_TEST_FAIL_CHUNK", "NARRATE_TEST_CRASH_CHUNK", "NARRATE_TEST_WATERMARK_FAIL"].contains(&up.as_str()),
                "only the three worker hooks may pass: {name}"
            );
        }
    }

    // Offline and redirected caches (spec-narrate-setup.md, Child process environment).
    let d = ready.d();
    let expect = |name: &str, value: String| {
        assert_eq!(env_get(&dump, name), Some(value), "{name}");
    };
    expect("HF_HUB_OFFLINE", "1".into());
    expect("TRANSFORMERS_OFFLINE", "1".into());
    expect("HF_HOME", d.join("hf-home").display().to_string());
    expect("HF_HUB_CACHE", d.join("hf-home").join("hub").display().to_string());
    expect("HF_XET_CACHE", d.join("hf-home").join("xet").display().to_string());
    expect("HF_ASSETS_CACHE", d.join("hf-home").join("assets").display().to_string());
    expect("HF_HUB_DISABLE_TELEMETRY", "1".into());
    expect("DO_NOT_TRACK", "1".into());
    expect("HF_HUB_DISABLE_PROGRESS_BARS", "1".into());
    expect("HF_HUB_DISABLE_SYMLINKS_WARNING", "1".into());
    expect("HF_HUB_DISABLE_XET", "1".into());
    expect("TORCH_HOME", d.join("torch-home").display().to_string());
    expect("NUMBA_CACHE_DIR", d.join("numba-cache").display().to_string());
    expect("CUDA_CACHE_PATH", d.join("cuda-cache").display().to_string());
    expect("MPLCONFIGDIR", d.join("mpl-config").display().to_string());
    expect("TQDM_DISABLE", "1".into());

    // Pass-through variables.
    expect("HTTPS_PROXY", "http://127.0.0.1:9".into());
    expect("HTTP_PROXY", "http://127.0.0.1:9".into());
    expect("CUDA_VISIBLE_DEVICES", "-1".into());
    expect("FAKE_WORKER_PASSTHROUGH", "yes".into());
    expect("NARRATE_TEST_FAIL_CHUNK", "99".into());
    expect("NARRATE_TEST_CRASH_CHUNK", "99".into());
    assert!(env_get(&dump, "PATH").is_some(), "PATH passes through");
    assert_eq!(env_get(&dump, "NARRATE_TEST_WATERMARK_FAIL"), None, "unset hooks are not invented");

    // Working directory is the data directory.
    assert!(same_path(dump["cwd"].as_str().unwrap(), &d), "cwd {}", dump["cwd"]);
}

// [Scenario: Worker environment is offline | spec-narrate-generation Req 14 | NFR-002]
#[test]
fn worker_temp_directory_is_a_per_run_directory_under_data_tmp_and_is_removed() {
    let ready = Ready::new();
    let work = scratch();
    let script = script_file(work.path(), "one.txt", "Hello there.");
    let dump_path = work.path().join("env.json");
    ready.nar().env("FAKE_WORKER_ENV_DUMP", &dump_path).arg(&script).run().assert_code(0);
    let dump = read_json(&dump_path);
    let temp = env_get(&dump, "TEMP").expect("TEMP");
    let tmp = env_get(&dump, "TMP").expect("TMP");
    assert_eq!(temp, tmp, "TEMP and TMP name the same directory");
    let temp_path = PathBuf::from(&temp);
    let parent = temp_path.parent().expect("parent");
    assert!(same_path(&parent.display().to_string(), &ready.d().join("tmp")), "directly under D\\tmp: {temp}");
    let run = temp_path.file_name().unwrap().to_string_lossy().into_owned();
    let (pid, millis) = run.split_once('-').unwrap_or_else(|| panic!("<narrate pid>-<unix ms>: {run}"));
    assert!(!pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()), "{run}");
    assert!(!millis.is_empty() && millis.bytes().all(|b| b.is_ascii_digit()), "{run}");
    assert_eq!(dump["temp_exists"], true, "the directory exists while the worker runs");
    assert!(!temp_path.exists(), "the run directory is removed when narrate exits");
    assert!(!ready.path().join("tmp").exists(), "D\\tmp is removed too when it is left empty");
}

// [Scenario: Worker environment is offline, failure path | spec-narrate-generation Req 14 | NFR-002]
#[test]
fn run_temp_directory_is_removed_after_a_failed_run_too() {
    let ready = Ready::new();
    let work = scratch();
    let script = script_file(work.path(), "one.txt", "Hello there.");
    let dump_path = work.path().join("env.json");
    ready
        .nar()
        .env("FAKE_WORKER_ENV_DUMP", &dump_path)
        .env("NARRATE_TEST_FAIL_CHUNK", "1")
        .arg(&script)
        .run()
        .assert_code(1);
    let temp = env_get(&read_json(&dump_path), "TEMP").expect("TEMP");
    assert!(!Path::new(&temp).exists());
    assert!(!ready.path().join("tmp").exists());
}

// ---------------------------------------------------------------------------------------
// Housekeeping of files narrate owns (spec-narrate-setup.md, Child process environment;
// spec-narrate-generation.md Requirement 13 last paragraph)
// ---------------------------------------------------------------------------------------

// [Child process environment: stale temporary directories | spec-narrate-setup | NFR-002]
#[test]
fn stale_tmp_entries_are_deleted_after_the_readiness_check_and_recent_ones_stay() {
    let ready = Ready::new();
    let work = scratch();
    let script = script_file(work.path(), "one.txt", "Hello there.");
    let tmp = ready.path().join("tmp");
    fs::create_dir_all(&tmp).unwrap();
    let old = write_text(&tmp, "old.bin", "stale");
    let new = write_text(&tmp, "new.bin", "recent");
    age_file(&old, 25);
    ready.nar().arg(&script).run().assert_code(0);
    assert!(!old.exists(), "entries older than 24 hours are removed");
    assert!(new.exists(), "recent entries protect the temporary directory of a running generation");
    assert!(tmp.is_dir(), "D\\tmp stays while it is not empty");
}

// [Requirement 13: staging leftovers | spec-narrate-generation | FR-005]
#[test]
fn old_trimmed_clip_leftovers_in_staging_are_deleted_and_other_files_stay() {
    let ready = Ready::new();
    let work = scratch();
    let script = script_file(work.path(), "one.txt", "Hello there.");
    let staging = ready.path().join("staging");
    fs::create_dir_all(&staging).unwrap();
    let old = write_bytes(&staging, "voice-111.wav", b"old");
    let young = write_bytes(&staging, "voice-222.wav", b"young");
    let not_a_clip = write_text(&staging, "voice-333.txt", "not a wav");
    let other = write_text(&staging, "keep.txt", "keep me");
    age_file(&old, 2);
    age_file(&not_a_clip, 2);
    age_file(&other, 2);
    ready.nar().arg(&script).run().assert_code(0);
    assert!(!old.exists(), "staging\\voice-*.wav older than 1 hour is deleted");
    assert!(young.exists());
    assert!(not_a_clip.exists());
    assert!(other.exists());
}

// [Business rule RULE-004 | spec-narrate-setup | UR-010]
#[test]
fn generation_never_touches_the_reserved_onnx_directory() {
    let ready = Ready::new();
    let work = scratch();
    let script = script_file(work.path(), "one.txt", "Hello there.");
    let onnx = ready.path().join("onnx");
    fs::create_dir_all(&onnx).unwrap();
    let keep = write_text(&onnx, "model.bin", "phase 2 data");
    age_file(&keep, 24 * 30);
    ready.nar().arg(&script).run().assert_code(0);
    assert_eq!(fs::read_to_string(&keep).unwrap(), "phase 2 data");
}
