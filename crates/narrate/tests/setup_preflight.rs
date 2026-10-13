//! Setup preflight: uv presence and version, the source allow-list, the lock scanner, the pin
//! rules and the data directory checks (FR-010, FR-011, BR-003, UR-010).
//!
//! Sources: specs/spec-narrate-setup.md (Requirement 1 and its Scenarios, Source allow-list,
//! Preconditions), specs/spec-narrate-cli.md (error catalog). Every case runs `narrate setup`
//! against an empty scratch data directory with the fake uv, which logs its invocations; a
//! preflight failure must leave the directory empty and the fake uv's log with only the
//! `--version` probe. The lock and pin variants are derived at run time from the committed
//! `python/uv.lock` and `crates/narrate/pins.json`. No implementation was read.

mod common;

use common::*;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const FIX_RESTORE: &str = "restore python/uv.lock and pins.json from the narrate release";
const UV_FIX_ZIP: &str = "download https://github.com/astral-sh/uv/releases/download/0.10.2/uv-x86_64-pc-windows-msvc.zip, check that its sha256 is 493ebbe0e06128d6ee4905e1ed5e2a433fb0f7cfc08b0eaca9fab4ca76778ae1, and put uv.exe on PATH";
const UV_FIX_PS: &str = "or run: powershell -ExecutionPolicy ByPass -c \"irm https://astral.sh/uv/0.10.2/install.ps1 | iex\"";

struct Pre {
    data: TempDir,
    kit: TempDir,
}

impl Pre {
    fn new() -> Pre {
        Pre { data: scratch(), kit: scratch() }
    }

    fn uv_log(&self) -> PathBuf {
        self.kit.path().join("uv.log")
    }

    fn nar(&self) -> Nar {
        Nar::new(self.data.path()).uv().env("FAKE_UV_LOG", self.uv_log()).arg("setup")
    }

    fn d(&self) -> PathBuf {
        abs(self.data.path())
    }

    fn assert_only_the_version_probe_ran(&self) {
        let log = read_lossy(&self.uv_log());
        let lines: Vec<&str> = log.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
        assert_eq!(lines, vec!["--version"], "the fake uv must be called only for --version");
    }

    fn assert_nothing_created(&self) {
        assert!(is_empty_dir(self.data.path()), "preflight creates nothing: {:?}", list_dir(self.data.path()));
    }
}

// ---------------------------------------------------------------------------------------
// uv presence and version
// ---------------------------------------------------------------------------------------

// [Scenario: uv missing | spec-narrate-setup Req 1 | FR-010 | UR-003 | NFR-007]
#[test]
fn uv_missing_from_path_exits_5_with_both_install_hints_and_creates_nothing() {
    let data = scratch();
    let empty_bin = scratch();
    let out = Nar::new(data.path()).env("PATH", empty_bin.path()).arg("setup").run();
    out.assert_code(5);
    let lines = out.stderr_lines();
    let idx = lines
        .iter()
        .position(|l| *l == "error[uv-not-found]: uv was not found on PATH")
        .unwrap_or_else(|| panic!("{}", out.context()));
    assert_eq!(lines[idx + 1], format!("  fix: {UV_FIX_ZIP}"));
    assert_eq!(lines[idx + 2], format!("  fix: {UV_FIX_PS}"), "two fix lines, zip first");
    out.assert_stderr_contains("0.10.2");
    assert!(is_empty_dir(data.path()), "the scratch directory must stay empty");
    assert_eq!(out.stdout, "");
}

// [Error catalog: uv-not-found with NARRATE_UV | spec-narrate-cli | FR-010]
#[test]
fn narrate_uv_naming_a_missing_file_reports_the_path() {
    let data = scratch();
    let work = scratch();
    let missing = work.path().join("no-such-uv.exe");
    let out = Nar::new(data.path()).env("NARRATE_UV", &missing).arg("setup").run();
    out.assert_code(5);
    assert!(
        out.has_stderr_line(&format!("error[uv-not-found]: uv was not found at {}", missing.display())),
        "{}",
        out.context()
    );
    assert!(out.has_stderr_line(&format!("  fix: {UV_FIX_ZIP}")));
    assert!(is_empty_dir(data.path()));
}

// [Scenario: uv version mismatch | spec-narrate-setup Req 1 | FR-010 | FR-011 | NFR-007]
#[test]
fn wrong_uv_version_exits_5_names_both_versions_and_prints_both_hints_in_order() {
    let pre = Pre::new();
    let out = pre.nar().env("FAKE_UV_VERSION", "0.13.0").run();
    out.assert_code(5);
    assert!(
        out.has_stderr_line("error[uv-version]: uv 0.13.0 is installed, but narrate needs uv 0.10.2"),
        "{}",
        out.context()
    );
    for needle in [
        "0.13.0",
        "0.10.2",
        "uv-x86_64-pc-windows-msvc.zip",
        "493ebbe0e06128d6ee4905e1ed5e2a433fb0f7cfc08b0eaca9fab4ca76778ae1",
        "install.ps1",
    ] {
        out.assert_stderr_contains(needle);
    }
    let zip_at = out.stderr_line_index(&format!("  fix: {UV_FIX_ZIP}")).expect("zip fix line");
    let ps_at = out.stderr_line_index(&format!("  fix: {UV_FIX_PS}")).expect("install.ps1 fix line");
    assert!(zip_at < ps_at, "the zip line comes before the install.ps1 line");
    pre.assert_only_the_version_probe_ran();
    pre.assert_nothing_created();
}

// [Scenario: uv version mismatch, boundary versions | spec-narrate-setup Req 1 | FR-011]
#[test]
fn uv_versions_that_only_resemble_the_pinned_one_are_rejected() {
    for version in ["0.10.20", "0.10.1", "0.10.3", "0.1.0", "1.10.2", "0.10"] {
        let pre = Pre::new();
        let out = pre.nar().env("FAKE_UV_VERSION", version).run();
        out.assert_code(5);
        assert!(
            out.has_stderr_line(&format!("error[uv-version]: uv {version} is installed, but narrate needs uv 0.10.2")),
            "{version}\n{}",
            out.context()
        );
        pre.assert_nothing_created();
    }
}

// [Preconditions: uv present via PATH | spec-narrate-setup | FR-010]
#[test]
fn uv_exe_is_found_by_searching_the_path_directories() {
    let pre = Pre::new();
    let bin = scratch();
    fs::copy(uv_exe(), bin.path().join("uv.exe")).expect("copy fake uv to uv.exe");
    // No NARRATE_UV: only the PATH search can find it. The version 0.13.0 stops preflight.
    let out = Nar::new(pre.data.path())
        .env("PATH", bin.path())
        .env("FAKE_UV_VERSION", "0.13.0")
        .env("FAKE_UV_LOG", pre.uv_log())
        .arg("setup")
        .run();
    out.assert_code(5);
    assert!(
        out.has_stderr_line("error[uv-version]: uv 0.13.0 is installed, but narrate needs uv 0.10.2"),
        "{}",
        out.context()
    );
    pre.assert_only_the_version_probe_ran();
}

// [Requirement: verbose setup output | spec-narrate-setup Algorithmic steps | FR-010 | FR-024]
#[test]
fn verbose_prints_the_pid_of_the_preflight_uv_child() {
    let pre = Pre::new();
    let out = pre.nar().arg("--verbose").env("FAKE_UV_VERSION", "0.13.0").run();
    out.assert_code(5);
    let line = out.find_stderr_prefix("verbose: uv pid ").unwrap_or_else(|| panic!("{}", out.context()));
    assert!(line["verbose: uv pid ".len()..].bytes().all(|b| b.is_ascii_digit()), "{line}");
    pre.assert_nothing_created();
}

// ---------------------------------------------------------------------------------------
// Lock scanner (fail-closed line scanner over the committed uv.lock)
// ---------------------------------------------------------------------------------------

fn lock_lines() -> Vec<String> {
    read_lossy(&real_lock_path()).lines().map(|l| l.to_string()).collect()
}

fn write_lock(dir: &Path, lines: &[String]) -> PathBuf {
    write_text(dir, "uv-variant.lock", &(lines.join("\n") + "\n"))
}

/// `(index of the [[package]] header, index of the name line, exclusive end of the main part)`.
fn find_block(lines: &[String], name: &str) -> (usize, usize, usize) {
    let want = format!("name = \"{name}\"");
    for i in 1..lines.len() {
        if lines[i] == want && lines[i - 1] == "[[package]]" {
            let mut end = i + 1;
            while end < lines.len() && !lines[end].starts_with('[') {
                end += 1;
            }
            return (i - 1, i, end);
        }
    }
    panic!("package {name} not found in python/uv.lock");
}

fn line_with_prefix(lines: &[String], from: usize, to: usize, prefix: &str) -> usize {
    (from..to)
        .find(|i| lines[*i].starts_with(prefix))
        .unwrap_or_else(|| panic!("no line starting with {prefix:?} in lines {from}..{to}"))
}

const PYPI_FILES: &str = "https://files.pythonhosted.org/packages/";

/// Index of the first artifact line served from files.pythonhosted.org, and the URL on it.
fn first_pypi_artifact(lines: &[String]) -> (usize, String) {
    for (i, l) in lines.iter().enumerate() {
        if l.contains("url = \"") && !l.starts_with("source = ") {
            if let Some(start) = l.find(PYPI_FILES) {
                let end = l[start..].find('"').map(|e| e + start).expect("closing quote");
                return (i, l[start..end].to_string());
            }
        }
    }
    panic!("no files.pythonhosted.org artifact in python/uv.lock");
}

fn package_name_before(lines: &[String], idx: usize) -> String {
    for i in (0..idx).rev() {
        if let Some(rest) = lines[i].strip_prefix("name = \"") {
            return rest.trim_end_matches('"').to_string();
        }
    }
    panic!("no package name before line {idx}");
}

fn run_with_lock(pre: &Pre, lines: &[String]) -> Out {
    let lock = write_lock(pre.kit.path(), lines);
    pre.nar().env("NARRATE_TEST_LOCK_FILE", &lock).run()
}

fn assert_preflight_stopped(pre: &Pre) {
    pre.assert_only_the_version_probe_ran();
    pre.assert_nothing_created();
}

// [Scenario: Disallowed source | spec-narrate-setup Req 1 | FR-011 | BR-003 | UR-010]
#[test]
fn a_wheel_url_outside_the_allow_list_is_refused_before_anything_is_created() {
    let pre = Pre::new();
    let mut lines = lock_lines();
    let (k, url) = first_pypi_artifact(&lines);
    let evil = "https://evil.example.com/pkg-1.0-py3-none-any.whl";
    lines[k] = lines[k].replace(&url, evil);
    let out = run_with_lock(&pre, &lines);
    out.assert_error(5, "source-not-allowed", &format!("{evil} is not an allowed download source"), FIX_RESTORE);
    out.assert_stderr_contains("https://evil.example.com/pkg-1.0-py3-none-any.whl");
    assert_preflight_stopped(&pre);
}

// [Requirement 1: URLs must use https | spec-narrate-setup | FR-011 | BR-003]
#[test]
fn a_plain_http_url_is_refused_even_on_an_allowed_host() {
    let pre = Pre::new();
    let mut lines = lock_lines();
    let (k, url) = first_pypi_artifact(&lines);
    let insecure = url.replacen("https://", "http://", 1);
    lines[k] = lines[k].replace(&url, &insecure);
    let out = run_with_lock(&pre, &lines);
    out.assert_error(5, "source-not-allowed", &format!("{insecure} is not an allowed download source"), FIX_RESTORE);
    assert_preflight_stopped(&pre);
}

// [Requirement 1: URL substrings everywhere | spec-narrate-setup | FR-011 | BR-003]
#[test]
fn a_foreign_url_hidden_in_a_metadata_line_is_refused_too() {
    let pre = Pre::new();
    let mut lines = lock_lines();
    let evil = "https://evil.example.com/index";
    lines.push(String::new());
    lines.push("[package.metadata]".to_string());
    lines.push(format!("index = \"{evil}\""));
    let out = run_with_lock(&pre, &lines);
    out.assert_error(5, "source-not-allowed", &format!("{evil} is not an allowed download source"), FIX_RESTORE);
    assert_preflight_stopped(&pre);
}

// [Scenario: Unhashed artifact | spec-narrate-setup Req 1 | FR-011 | NFR-001]
#[test]
fn an_artifact_without_a_sha256_hash_is_pin_invalid() {
    let pre = Pre::new();
    let mut lines = lock_lines();
    let (k, _) = first_pypi_artifact(&lines);
    let line = lines[k].clone();
    let marker = "hash = \"sha256:";
    let h = line.find(marker).expect("artifact has a hash");
    let end = h + marker.len() + 64 + 1;
    let mut start = h;
    if line[..h].ends_with(", ") {
        start -= 2;
    }
    lines[k] = format!("{}{}", &line[..start], &line[end..]);
    let package = package_name_before(&lines, k);
    let out = run_with_lock(&pre, &lines);
    out.assert_error(
        5,
        "pin-invalid",
        &format!("uv.lock line {} (package {package}) has an artifact without a sha256 hash", k + 1),
        FIX_RESTORE,
    );
    assert_preflight_stopped(&pre);
}

// [Scenario: Torch from the wrong index | spec-narrate-setup Req 1 | FR-011 | BR-003]
#[test]
fn torch_from_pypi_is_refused_with_the_exact_cause() {
    let pre = Pre::new();
    let mut lines = lock_lines();
    let (_, name_at, end) = find_block(&lines, "torch");
    let s = line_with_prefix(&lines, name_at, end, "source = ");
    lines[s] = "source = { registry = \"https://pypi.org/simple\" }".to_string();
    let out = run_with_lock(&pre, &lines);
    out.assert_error(
        5,
        "source-not-allowed",
        "torch must come from https://download.pytorch.org/whl/ (the lock uses https://pypi.org/simple)",
        FIX_RESTORE,
    );
    assert_preflight_stopped(&pre);
}

// [Scenario: Other package from the PyTorch index | spec-narrate-setup Req 1 | FR-011 | BR-003]
#[test]
fn numpy_from_the_pytorch_index_is_refused_with_the_exact_cause() {
    let pre = Pre::new();
    let mut lines = lock_lines();
    let (_, name_at, end) = find_block(&lines, "numpy");
    let s = line_with_prefix(&lines, name_at, end, "source = ");
    lines[s] = "source = { registry = \"https://download.pytorch.org/whl/cu124\" }".to_string();
    let out = run_with_lock(&pre, &lines);
    out.assert_error(
        5,
        "source-not-allowed",
        "numpy may not come from https://download.pytorch.org/whl/",
        FIX_RESTORE,
    );
    assert_preflight_stopped(&pre);
}

// [Requirement 1: git, path, directory and editable sources | spec-narrate-setup | FR-011 | BR-003]
#[test]
fn non_registry_source_kinds_are_refused_with_their_kind() {
    for (kind, source) in [
        ("git", "source = { git = \"numpy-fork\" }"),
        ("path", "source = { path = \"../numpy\" }"),
        ("directory", "source = { directory = \"numpy\" }"),
        ("editable", "source = { editable = \".\" }"),
    ] {
        let pre = Pre::new();
        let mut lines = lock_lines();
        let (_, name_at, end) = find_block(&lines, "numpy");
        let s = line_with_prefix(&lines, name_at, end, "source = ");
        lines[s] = source.to_string();
        let out = run_with_lock(&pre, &lines);
        out.assert_error(
            5,
            "source-not-allowed",
            &format!("numpy uses a {kind} source, which is not allowed"),
            FIX_RESTORE,
        );
        assert_preflight_stopped(&pre);
    }
}

// [Scenario: Unrecognized lock line | spec-narrate-setup Req 1 | FR-011 | NFR-001]
#[test]
fn an_unrecognized_line_in_a_package_block_is_pin_invalid() {
    let pre = Pre::new();
    let mut lines = lock_lines();
    let (_, name_at, _) = find_block(&lines, "numpy");
    lines.insert(name_at + 1, "!!garbage".to_string());
    let out = run_with_lock(&pre, &lines);
    out.assert_error(
        5,
        "pin-invalid",
        &format!("uv.lock line {} has an unrecognized form", name_at + 2),
        FIX_RESTORE,
    );
    out.assert_stderr_contains("has an unrecognized form");
    assert_preflight_stopped(&pre);
}

// [Requirement 1: completeness | spec-narrate-setup | FR-011 | NFR-001]
#[test]
fn a_package_block_missing_version_or_source_is_pin_invalid() {
    for field in ["version", "source"] {
        let pre = Pre::new();
        let mut lines = lock_lines();
        let (_, name_at, end) = find_block(&lines, "numpy");
        let idx = line_with_prefix(&lines, name_at, end, &format!("{field} = "));
        lines.remove(idx);
        let out = run_with_lock(&pre, &lines);
        out.assert_error(5, "pin-invalid", &format!("uv.lock package numpy is missing its {field}"), FIX_RESTORE);
        assert_preflight_stopped(&pre);
    }
}

// [Requirement 1: completeness, missing name | spec-narrate-setup | FR-011 | NFR-001]
#[test]
fn a_package_block_without_a_name_is_pin_invalid_and_located_by_line() {
    let pre = Pre::new();
    let mut lines = lock_lines();
    let (_, name_at, _) = find_block(&lines, "numpy");
    lines.remove(name_at);
    let out = run_with_lock(&pre, &lines);
    out.assert_code(5);
    assert!(
        out.stderr.lines().any(|l| l.starts_with("error[pin-invalid]: uv.lock package at line ") && l.ends_with(" is missing its name")),
        "{}",
        out.context()
    );
    assert!(out.has_stderr_line(&format!("  fix: {FIX_RESTORE}")));
    assert_preflight_stopped(&pre);
}

// ---------------------------------------------------------------------------------------
// Pin rules (pins.json)
// ---------------------------------------------------------------------------------------

fn run_with_pins(pre: &Pre, mutate: impl FnOnce(&mut Value)) -> Out {
    let pins = write_pins_variant(pre.kit.path(), "pins-variant.json", mutate);
    pre.nar().env("NARRATE_TEST_PINS_FILE", &pins).run()
}

// [Scenario: Unsafe pin path | spec-narrate-setup Req 1 | FR-011 | BR-003 | UR-010]
#[test]
fn a_file_path_that_escapes_the_directory_is_pin_invalid_and_nothing_is_created() {
    let pre = Pre::new();
    let out = run_with_pins(&pre, |v| v["huggingface"][0]["files"][0]["path"] = json!("../evil.bin"));
    out.assert_error(5, "pin-invalid", "pins.json path ../evil.bin is not a safe relative path", FIX_RESTORE);
    out.assert_stderr_contains("../evil.bin");
    assert_preflight_stopped(&pre);
}

// [Requirement 1: safe relative paths | spec-narrate-setup | FR-011 | UR-010]
#[test]
fn every_kind_of_unsafe_pin_path_is_rejected() {
    for bad in ["/abs.bin", "a\\b.bin", "C:evil.bin", "a//b", "./x", "a/./b", "a/../b", "..", "a/"] {
        let pre = Pre::new();
        let out = run_with_pins(&pre, |v| v["huggingface"][0]["files"][1]["path"] = json!(bad));
        out.assert_error(5, "pin-invalid", &format!("pins.json path {bad} is not a safe relative path"), FIX_RESTORE);
        assert_preflight_stopped(&pre);
    }
}

// [Requirement 1: dir is a path too | spec-narrate-setup | FR-011 | UR-010]
#[test]
fn an_unsafe_repo_directory_is_rejected_like_an_unsafe_file_path() {
    for bad in ["..", "../outside", "C:\\x", "/rooted"] {
        let pre = Pre::new();
        let out = run_with_pins(&pre, |v| v["huggingface"][1]["dir"] = json!(bad));
        out.assert_error(5, "pin-invalid", &format!("pins.json path {bad} is not a safe relative path"), FIX_RESTORE);
        assert_preflight_stopped(&pre);
    }
}

// [Requirement 1: pin rules, revision | spec-narrate-setup | FR-011 | NFR-001]
#[test]
fn a_revision_that_is_not_forty_lowercase_hex_digits_is_pin_invalid() {
    let pre = Pre::new();
    let out = run_with_pins(&pre, |v| v["huggingface"][0]["revision"] = json!("main"));
    out.assert_error(
        5,
        "pin-invalid",
        "pins.json huggingface[0].revision must be 40 lowercase hex digits, found main",
        FIX_RESTORE,
    );
    assert_preflight_stopped(&pre);

    for bad in ["A".repeat(40), "a".repeat(39), "a".repeat(41)] {
        let pre = Pre::new();
        let out = run_with_pins(&pre, |v| v["huggingface"][0]["revision"] = json!(bad.clone()));
        out.assert_error(
            5,
            "pin-invalid",
            &format!("pins.json huggingface[0].revision must be 40 lowercase hex digits, found {bad}"),
            FIX_RESTORE,
        );
    }
}

// [Requirement 1: pin rules, repositories | spec-narrate-setup | FR-011 | BR-003]
#[test]
fn a_repository_outside_the_allow_list_is_refused() {
    let pre = Pre::new();
    let out = run_with_pins(&pre, |v| v["huggingface"][0]["repo"] = json!("evil/model"));
    out.assert_error(5, "source-not-allowed", "Hugging Face repository evil/model is not allowed", FIX_RESTORE);
    assert_preflight_stopped(&pre);
}

// [Requirement 1: pin rules, python url | spec-narrate-setup | FR-011 | BR-003]
#[test]
fn a_python_url_outside_the_python_build_standalone_prefix_is_refused() {
    let pre = Pre::new();
    let evil = "https://evil.example.com/python.tar.gz";
    let out = run_with_pins(&pre, |v| v["python"]["url"] = json!(evil));
    out.assert_code(5);
    assert!(
        out.stderr.lines().any(|l| l.starts_with("error[source-not-allowed]: ") && l.contains(evil)),
        "{}",
        out.context()
    );
    assert!(out.has_stderr_line(&format!("  fix: {FIX_RESTORE}")));
    assert_preflight_stopped(&pre);
}

fn apply_rule_violation(case: usize, v: &mut Value) {
    match case {
        0 => v["python"]["sha256"] = json!("xyz"),
        1 => v["python"]["sha256"] = json!("A".repeat(64)),
        2 => v["huggingface"][0]["files"][0]["sha256"] = json!("abc"),
        3 => v["uv"]["version"] = json!("0.9.0"),
        4 => v["format"] = json!(2),
        _ => unreachable!(),
    }
}

// [Requirement 1: pin rules, hashes and versions | spec-narrate-setup | FR-011 | NFR-001]
#[test]
fn malformed_hashes_versions_and_formats_are_pin_invalid() {
    // (case, what, text the message must contain)
    let cases = [
        (0usize, "python sha256 not hex", "sha256"),
        (1, "python sha256 uppercase", "sha256"),
        (2, "file sha256 too short", "sha256"),
        (3, "uv version", "0.10.2"),
        (4, "format", "format"),
    ];
    for (case, what, needle) in cases {
        let pre = Pre::new();
        let out = run_with_pins(&pre, |v| apply_rule_violation(case, v));
        out.assert_code(5);
        let line = out
            .stderr
            .lines()
            .find(|l| l.starts_with("error[pin-invalid]: pins.json "))
            .unwrap_or_else(|| panic!("{what}: no pin-invalid line\n{}", out.context()))
            .to_string();
        assert!(line.contains(needle), "{what}: {line}");
        assert!(out.has_stderr_line(&format!("  fix: {FIX_RESTORE}")), "{what}");
        assert_preflight_stopped(&pre);
    }
}

fn apply_strictness_violation(case: usize, v: &mut Value) {
    match case {
        0 => v["extra"] = json!(1),
        1 => v["python"]["extra"] = json!(true),
        2 => v["huggingface"][0]["files"][0]["size"] = json!("big"),
        3 => v["format"] = json!("1"),
        4 => v["huggingface"][0]["files"] = json!({}),
        _ => unreachable!(),
    }
}

// [Requirement 1: pins parsed strictly | spec-narrate-setup | FR-011 | NFR-001]
#[test]
fn unknown_fields_wrong_types_and_broken_json_are_pin_invalid() {
    let cases = [
        (0usize, "unknown top-level field"),
        (1, "unknown nested field"),
        (2, "size as a string"),
        (3, "format as a string"),
        (4, "files not an array"),
    ];
    for (case, what) in cases {
        let pre = Pre::new();
        let out = run_with_pins(&pre, |v| apply_strictness_violation(case, v));
        out.assert_code(5);
        assert!(
            out.stderr.lines().any(|l| l.starts_with("error[pin-invalid]: pins.json is invalid: ")),
            "{what}\n{}",
            out.context()
        );
        assert_preflight_stopped(&pre);
    }

    for (what, text) in [("truncated JSON", "{"), ("empty file", ""), ("not JSON", "format: 1")] {
        let pre = Pre::new();
        let pins = write_text(pre.kit.path(), "bad-pins.json", text);
        let out = pre.nar().env("NARRATE_TEST_PINS_FILE", &pins).run();
        out.assert_code(5);
        assert!(
            out.stderr.lines().any(|l| l.starts_with("error[pin-invalid]: pins.json is invalid: ")),
            "{what}\n{}",
            out.context()
        );
        assert!(out.has_stderr_line(&format!("  fix: {FIX_RESTORE}")));
        assert_preflight_stopped(&pre);
    }
}

// ---------------------------------------------------------------------------------------
// Data directory preconditions
// ---------------------------------------------------------------------------------------

// [Scenario: Setup already running | spec-narrate-setup Req 1 | FR-010]
#[test]
fn a_held_setup_lock_exits_5_with_setup_busy() {
    let pre = Pre::new();
    let lock_path = pre.data.path().join("setup.lock");
    let held = fs::File::create(&lock_path).expect("create setup.lock");
    held.lock().expect("take an exclusive lock on setup.lock");
    let out = pre.nar().run();
    drop(held);
    out.assert_error(
        5,
        "setup-busy",
        &format!("another narrate setup is running for {}", pre.d().display()),
        "wait for it to finish, then run narrate setup again",
    );
    assert_eq!(list_dir(pre.data.path()), vec!["setup.lock".to_string()], "nothing else is created while the lock is held");
}

// [Preconditions: data directory writable | spec-narrate-setup | FR-010 | NFR-007]
#[test]
fn an_uncreatable_data_directory_exits_5_with_data_dir_unwritable() {
    let base = scratch();
    let blocker = write_text(base.path(), "afile", "i am a file, not a directory");
    let data = blocker.join("sub");
    let kit = scratch();
    let out = Nar::new(&data).uv().env("FAKE_UV_LOG", kit.path().join("uv.log")).arg("setup").run();
    out.assert_error_prefix(
        5,
        "data-dir-unwritable",
        &format!("cannot write to the data directory {}: ", abs(&data).display()),
        "set NARRATE_DATA_DIR to a writable directory",
    );
    assert_eq!(fs::read_to_string(&blocker).unwrap(), "i am a file, not a directory");
}

// [Requirement 1: nothing is created before the checks pass | spec-narrate-setup | FR-011 | RULE-001]
#[test]
fn a_nonexistent_data_directory_is_not_created_by_a_failing_preflight() {
    let base = scratch();
    let data = base.path().join("not-yet");
    let kit = scratch();
    let out = Nar::new(&data)
        .uv()
        .env("FAKE_UV_VERSION", "0.13.0")
        .env("FAKE_UV_LOG", kit.path().join("uv.log"))
        .arg("setup")
        .run();
    out.assert_code(5);
    assert!(!data.exists(), "no D, D\\tmp or D\\logs may be created by a failed preflight");
}
