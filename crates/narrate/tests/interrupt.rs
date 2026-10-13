//! Interruption and orphan prevention: Ctrl+Break during generation and a killed parent
//! (FR-005, NFR-007, NFR-008).
//!
//! Sources: specs/spec-narrate-generation.md (Requirement 13, RULE-006), specs/spec-narrate-cli.md
//! (`interrupted`, exit 130), specs/spec-narrate-test-strategy.md (Test kit: `narrate-test-signal`).
//!
//! `narrate` is started with `CREATE_NEW_PROCESS_GROUP` (0x00000200) through
//! `std::os::windows::process::CommandExt::creation_flags`, so its process id is the group id.
//! The tests keep the `Child` handle themselves (they must call `Child::kill` for the
//! parent-killed case), so they do not go through `narrate-test-signal spawn-group`, which is
//! meant for PowerShell scripts that cannot set the flag. `GenerateConsoleCtrlEvent` only
//! reaches processes that share the helper's console, so these tests need `cargo test` to run
//! from a console session.

mod common;

use common::*;
use std::path::Path;
use std::time::{Duration, Instant};

/// File name of the custom voice clip the tests place next to the script. It is the user's
/// input, not a leftover of the run.
const INPUT_CLIP: &str = "my.wav";

struct Running {
    narrate: Spawned,
    ready: Ready,
    work: TempDir,
    _kit: TempDir,
    worker_pid: u64,
}

/// Starts a generation whose worker sleeps 30 s inside every `synth`, waits until the first
/// `synth` request is logged, and returns the running process plus the worker pid.
fn start(clip_frames: Option<u64>) -> Running {
    let ready = Ready::new();
    let work = scratch();
    let kit = scratch();
    let log = kit.path().join("worker.log");
    let script = write_text(work.path(), "slow.txt", "Hello there. This takes a long time.");
    let mut nar = ready
        .nar()
        .log(&log)
        .env("FAKE_WORKER_SYNTH_DELAY_MS", "30000")
        .arg(&script)
        .arg("--verbose");
    if let Some(frames) = clip_frames {
        let clip = write_bytes(work.path(), INPUT_CLIP, &pcm16_clip(24_000, frames));
        nar = nar.arg("--voice-file").arg(clip);
    }
    let narrate = nar.spawn_group(kit.path());
    let saw_synth = wait_until(Duration::from_secs(60), || !requests_of(&log, "synth").is_empty());
    assert!(
        saw_synth,
        "the first synth request was never logged\nstderr so far:\n{}",
        narrate.stderr()
    );
    let worker_pid = worker_pids(&log)[0];
    assert!(pid_alive(worker_pid), "the worker must be running while it is synthesizing");
    Running { narrate, ready, work, _kit: kit, worker_pid }
}

fn assert_worker_gone_within(pid: u64, since: Instant, limit: Duration) {
    let deadline = limit.saturating_sub(since.elapsed());
    assert!(
        wait_until(deadline, || !pid_alive(pid)),
        "worker {pid} was still running {limit:?} after the event"
    );
}

/// No output `.wav`, no `*.narrate-*.tmp` file. The user's own input clip is not a leftover.
fn assert_nothing_left(work: &Path) {
    let leftovers: Vec<String> = wav_or_tmp_files(work)
        .into_iter()
        .filter(|n| !n.eq_ignore_ascii_case(INPUT_CLIP))
        .collect();
    assert!(
        leftovers.is_empty(),
        "no output .wav and no *.narrate-*.tmp file: {:?}",
        list_dir(work)
    );
}

fn staging_clips(ready: &Ready) -> Vec<String> {
    list_dir(&ready.path().join("staging"))
        .into_iter()
        .filter(|n| n.starts_with("voice-"))
        .collect()
}

// [Scenario: Ctrl+Break during generation | spec-narrate-generation Req 13 | FR-005 | NFR-008]
#[test]
fn ctrl_break_ends_the_run_with_130_interrupted_and_no_worker_or_files_remain() {
    let mut run = start(None);
    let sent_at = Instant::now();
    assert!(
        send_ctrl_break(run.narrate.pid()),
        "narrate-test-signal could not deliver CTRL_BREAK_EVENT; run the tests from a console session"
    );
    let code = run.narrate.wait(Duration::from_secs(10));
    assert_eq!(
        code,
        Some(130),
        "narrate must exit 130 within 10 s\nstderr:\n{}",
        run.narrate.stderr()
    );
    let stderr = run.narrate.stderr();
    assert!(
        stderr.lines().any(|l| l == "error[interrupted]: interrupted; no output was written"),
        "{stderr}"
    );
    assert!(stderr.lines().any(|l| l == "  fix: run the command again"), "{stderr}");
    assert!(
        !stderr.contains("error[worker-exited]"),
        "an interrupt is never reported as a worker crash\n{stderr}"
    );
    assert_eq!(run.narrate.stdout(), "", "no path is printed for an interrupted run");
    assert_worker_gone_within(run.worker_pid, sent_at, Duration::from_secs(10));
    assert_nothing_left(run.work.path());
}

// [Scenario: Ctrl+Break during generation, custom clip | spec-narrate-generation Req 13 | FR-005 | FR-009]
#[test]
fn ctrl_break_also_removes_the_trimmed_clip_from_staging() {
    let mut run = start(Some(288_000)); // 12 s clip: a 10 s prefix lives in staging
    let sent_at = Instant::now();
    assert!(send_ctrl_break(run.narrate.pid()), "CTRL_BREAK_EVENT could not be delivered");
    assert_eq!(run.narrate.wait(Duration::from_secs(10)), Some(130), "{}", run.narrate.stderr());
    assert_worker_gone_within(run.worker_pid, sent_at, Duration::from_secs(10));
    assert_nothing_left(run.work.path());
    assert!(staging_clips(&run.ready).is_empty(), "{:?}", staging_clips(&run.ready));
    assert!(run.work.path().join(INPUT_CLIP).is_file(), "the user's clip is never deleted");
}

// [Scenario: Parent killed | spec-narrate-generation Req 13 | FR-005 | NFR-002]
#[test]
fn killing_narrate_terminates_the_worker_and_leaves_no_output() {
    let mut run = start(None);
    let killed_at = Instant::now();
    run.narrate.child.kill().expect("terminate narrate");
    let _ = run.narrate.wait(Duration::from_secs(10));
    assert_worker_gone_within(run.worker_pid, killed_at, Duration::from_secs(10));
    assert_nothing_left(run.work.path());
}

// [Scenario: Parent killed, custom clip | spec-narrate-generation Req 13 | FR-005]
#[test]
fn killing_narrate_leaves_no_output_file_even_with_a_trimmed_clip_in_flight() {
    let mut run = start(Some(288_000));
    let killed_at = Instant::now();
    run.narrate.child.kill().expect("terminate narrate");
    let _ = run.narrate.wait(Duration::from_secs(10));
    assert_worker_gone_within(run.worker_pid, killed_at, Duration::from_secs(10));
    assert_nothing_left(run.work.path());
    // A hard kill may leave staging\voice-<pid>.wav behind; the next run removes it once it is
    // older than one hour (covered in chunks.rs), so nothing is asserted about staging here.
}

// [Requirement 13: worker is never reached by console events | spec-narrate-generation | FR-005]
#[test]
fn console_event_reaches_only_narrate_so_the_exit_is_interrupted_not_a_worker_crash() {
    // The worker has its own process group and no console, so the event reaches only narrate:
    // the observable consequence is exit code 130 with `interrupted` rather than `worker-exited`
    // (exit code 1).
    let mut run = start(None);
    assert!(send_ctrl_break(run.narrate.pid()), "CTRL_BREAK_EVENT could not be delivered");
    let code = run.narrate.wait(Duration::from_secs(10));
    assert_eq!(code, Some(130));
    assert!(!run.narrate.stderr().contains("exited unexpectedly"));
}
