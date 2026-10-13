//! Repository inspection: pins, lock, manifests, line endings and QA asset content
//! (FR-011, NFR-001, FR-018, NFR-004).
//!
//! Sources: specs/spec-narrate-test-strategy.md (Requirements: Repository inspection, Test kit,
//! QA assets, Test command and dev-dependencies), specs/spec-narrate-setup.md (Pin file schema,
//! Pinned model files, Source allow-list), specs/spec-narrate-architecture.md (Workspace
//! manifest, Crate manifests, Pinned inputs have stable line endings). Paths are relative to
//! `env!("CARGO_MANIFEST_DIR")`. No implementation body was read; these tests read committed
//! data files and manifests only.

mod common;

use common::*;
use std::fs;
use std::path::PathBuf;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repo_file(rel: &str) -> PathBuf {
    rel.split('/').fold(repo_root(), |acc, part| acc.join(part))
}

fn read_repo_text(rel: &str) -> String {
    let p = repo_file(rel);
    let bytes = fs::read(&p).unwrap_or_else(|e| panic!("{} must exist: {e}", p.display()));
    String::from_utf8(bytes).unwrap_or_else(|e| panic!("{} must be UTF-8: {e}", p.display()))
}

fn compact_text(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

// ---------------------------------------------------------------------------------------
// pins.json
// ---------------------------------------------------------------------------------------

/// The 13 rows of the "Pinned model files" table, in the table's order.
const MODEL_FILES: [(&str, &str, u64, &str); 13] = [
    ("ResembleAI/chatterbox-turbo", "t3_turbo_v1.safetensors", 1915480052, "fcf1f8c1d651bb7e3acd69ee5be269b4ac10c02980b7708213d598bc9f7cdf87"),
    ("ResembleAI/chatterbox-turbo", "s3gen_meanflow.safetensors", 1064875036, "d65cb687a2ed581ee6cc297e919ffefa63386944f42364ae13b78a594945514f"),
    ("ResembleAI/chatterbox-turbo", "ve.safetensors", 5695784, "f0921cab452fa278bc25cd23ffd59d36f816d7dc5181dd1bef9751a7fb61f63c"),
    ("ResembleAI/chatterbox-turbo", "conds.pt", 169454, "b1852099306fd6a7814eb9d0bd10186caba7249596cc23868f78a0eefbfa5033"),
    ("ResembleAI/chatterbox-turbo", "vocab.json", 999186, "f6bd25a65e4e63ca31360e9fb11c7e4f9a391a78385d640acd814092dd6eee4f"),
    ("ResembleAI/chatterbox-turbo", "merges.txt", 456318, "1ce1664773c50f3e0cc8842619a93edc4624525b728b188a9e0be33b7726adc5"),
    ("ResembleAI/chatterbox-turbo", "tokenizer_config.json", 3878, "bca16a2ac1ddbd78b8d6228f0031884cc74b6ea54b967d6f6d2ebae9ccde23e6"),
    ("ResembleAI/chatterbox-turbo", "special_tokens_map.json", 470, "92ba8063bf40aa163eadebbfe0de07c2aebe44cf0d4a9e8726580b0781fd2640"),
    ("ResembleAI/chatterbox-turbo", "added_tokens.json", 418, "72e4ab6acb0d9309ac3df4b526ae5fd80a2da5bc5ab7bb02d85096a374f69193"),
    ("hexgrad/Kokoro-82M", "kokoro-v1_0.pth", 327212226, "496dba118d1a58f5f3db2efc88dbdc216e0483fc89fe6e47ee1f2c53f18ad1e4"),
    ("hexgrad/Kokoro-82M", "config.json", 2351, "5abb01e2403b072bf03d04fde160443e209d7a0dad49a423be15196b9b43c17f"),
    ("hexgrad/Kokoro-82M", "voices/af_heart.pt", 523425, "0ab5709b8ffab19bfd849cd11d98f75b60af7733253ad0d67b12382a102cb4ff"),
    ("hexgrad/Kokoro-82M", "voices/am_michael.pt", 523435, "9a443b79a4b22489a5b0ab7c651a0bcd1a30bef675c28333f06971abbd47bd37"),
];

// [Scenario: Pins recorded (FR-011) | spec-narrate-test-strategy / spec-narrate-setup Pin file schema | FR-011 | NFR-001 | BR-003]
#[test]
fn pins_json_records_uv_python_and_the_two_repositories() {
    let pins = load_real_pins();
    assert_eq!(pins["format"], 1);
    assert_eq!(pins["uv"]["version"], "0.10.2");
    assert_eq!(pins["python"]["version"], "3.11.14");
    assert_eq!(pins["python"]["build"], "cpython-3.11.14-windows-x86_64-none");
    assert_eq!(
        pins["python"]["url"],
        "https://github.com/astral-sh/python-build-standalone/releases/download/20260203/cpython-3.11.14%2B20260203-x86_64-pc-windows-msvc-install_only_stripped.tar.gz"
    );
    assert_eq!(pins["python"]["sha256"], "851ba0c5a7c281912ca9bf68c340c69156e9e292de26f9068137f5650b537ff5");

    let repos = pins["huggingface"].as_array().expect("huggingface array");
    assert_eq!(repos.len(), 2);
    assert_eq!(repos[0]["repo"], "ResembleAI/chatterbox-turbo");
    assert_eq!(repos[0]["revision"], "749d1c1a46eb10492095d68fbcf55691ccf137cd");
    assert_eq!(repos[0]["dir"], "chatterbox-turbo");
    assert_eq!(repos[1]["repo"], "hexgrad/Kokoro-82M");
    assert_eq!(repos[1]["revision"], "f3ff3571791e39611d31c381e3a41a3af07b4987");
    assert_eq!(repos[1]["dir"], "kokoro-82m");
}

// [Scenario: Pins recorded (FR-011) | spec-narrate-setup Pinned model files | FR-011 | NFR-001]
#[test]
fn pins_json_lists_exactly_the_thirteen_pinned_files_with_sizes_and_hashes() {
    let pins = load_real_pins();
    let mut actual: Vec<(String, String, u64, String)> = Vec::new();
    for (repo, _dir, _rev, files) in pin_files(&pins) {
        for (path, size, sha) in files {
            actual.push((repo.clone(), path, size, sha));
        }
    }
    let expected: Vec<(String, String, u64, String)> = MODEL_FILES
        .iter()
        .map(|(r, p, s, h)| (r.to_string(), p.to_string(), *s, h.to_string()))
        .collect();
    assert_eq!(actual.len(), 13, "exactly 13 files");
    assert_eq!(actual, expected, "paths, sizes and full sha256 values in the table's order");
    assert!(!actual.iter().any(|(_, p, _, _)| p == "s3gen.safetensors"), "s3gen.safetensors is not fetched");
    for (_, _, _, sha) in &actual {
        assert_eq!(sha.len(), 64);
        assert!(sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "{sha}");
    }
}

// [Scenario: Pins recorded (FR-011) | spec-narrate-setup Reference text and voices | FR-010 | FR-011]
#[test]
fn pins_json_carries_the_reference_text_and_voice_recipes() {
    let pins = load_real_pins();
    let voices = &pins["voices"];
    assert_eq!(voices["reference_text"].as_str(), Some(REFERENCE_TEXT));
    assert_eq!(voices["reference_text"].as_str().unwrap().chars().count(), 223);
    assert_eq!(voices["min_samples"], 240_000);
    assert_eq!(voices["female"]["engine"], "kokoro");
    assert_eq!(voices["female"]["voice"], "af_heart");
    assert_eq!(voices["male"]["engine"], "kokoro");
    assert_eq!(voices["male"]["voice"], "am_michael");
    assert_eq!(voices["builtin"]["engine"], "chatterbox-turbo");
    assert_eq!(voices["builtin"]["seed"], 1);
}

// [Scenario: Pins recorded (FR-011) | docs/requirements FR-011 | FR-011 | NFR-001]
#[test]
fn every_pinned_revision_is_a_full_commit_hash() {
    let pins = load_real_pins();
    for repo in pins["huggingface"].as_array().unwrap() {
        let rev = repo["revision"].as_str().unwrap();
        assert_eq!(rev.len(), 40, "{rev}");
        assert!(rev.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "{rev}");
    }
}

// ---------------------------------------------------------------------------------------
// uv.lock
// ---------------------------------------------------------------------------------------

struct Pkg {
    name: String,
    version: String,
    source: String,
    artifacts: Vec<String>,
}

fn parse_lock(text: &str) -> Vec<Pkg> {
    let mut pkgs: Vec<Pkg> = Vec::new();
    let mut cur: Option<Pkg> = None;
    let mut in_main = false;
    for raw in text.lines() {
        let line = raw.trim_end();
        if line == "[[package]]" {
            if let Some(p) = cur.take() {
                pkgs.push(p);
            }
            cur = Some(Pkg { name: String::new(), version: String::new(), source: String::new(), artifacts: Vec::new() });
            in_main = true;
            continue;
        }
        if line.starts_with('[') {
            in_main = false;
            continue;
        }
        if !in_main {
            continue;
        }
        let Some(p) = cur.as_mut() else { continue };
        if let Some(rest) = line.strip_prefix("name = \"") {
            p.name = rest.trim_end_matches('"').to_string();
        } else if let Some(rest) = line.strip_prefix("version = \"") {
            p.version = rest.trim_end_matches('"').to_string();
        } else if line.starts_with("source = ") {
            p.source = line.to_string();
        } else if line.contains("url = \"") {
            p.artifacts.push(line.to_string());
        }
    }
    if let Some(p) = cur.take() {
        pkgs.push(p);
    }
    pkgs
}

fn has_sha256_hash(line: &str) -> bool {
    let marker = "hash = \"sha256:";
    let Some(at) = line.find(marker) else { return false };
    let rest = line[at + marker.len()..].as_bytes();
    rest.len() > 64 && rest[..64].iter().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b)) && rest[64] == b'"'
}

fn lock_packages() -> Vec<Pkg> {
    parse_lock(&read_repo_text("python/uv.lock"))
}

fn find_pkg<'a>(pkgs: &'a [Pkg], name: &str) -> &'a Pkg {
    pkgs.iter().find(|p| p.name == name).unwrap_or_else(|| panic!("python/uv.lock must list {name}"))
}

const PYTORCH: &str = "https://download.pytorch.org/whl/";

// [Scenario: Pins recorded (FR-011) | spec-narrate-test-strategy Repository inspection | FR-011 | NFR-001]
#[test]
fn lock_lists_the_pinned_chatterbox_and_build_tool_versions() {
    let pkgs = lock_packages();
    assert_eq!(find_pkg(&pkgs, "chatterbox-tts").version, "0.1.7");
    assert_eq!(find_pkg(&pkgs, "setuptools").version, "81.0.0");
    assert_eq!(find_pkg(&pkgs, "torch").version, "2.6.0+cu124");
    assert_eq!(find_pkg(&pkgs, "torchaudio").version, "2.6.0+cu124");
}

// [Scenario: Pins recorded (FR-011) | spec-narrate-setup Source allow-list | FR-011 | BR-003]
#[test]
fn torch_and_torchaudio_come_from_the_pytorch_index_and_nothing_else_does() {
    let pkgs = lock_packages();
    for name in ["torch", "torchaudio"] {
        let p = find_pkg(&pkgs, name);
        assert!(p.source.contains(PYTORCH), "{name} source {}", p.source);
        assert!(!p.artifacts.is_empty());
        for a in &p.artifacts {
            assert!(a.contains(PYTORCH), "{name} artifact {a}");
        }
    }
    for p in &pkgs {
        if p.name == "torch" || p.name == "torchaudio" {
            continue;
        }
        assert!(!p.source.contains(PYTORCH), "{} may not use the PyTorch index", p.name);
        for a in &p.artifacts {
            assert!(!a.contains(PYTORCH), "{} artifact {a}", p.name);
        }
    }
}

// [Scenario: Pins recorded (FR-011) | spec-narrate-setup Source allow-list (spaCy model) | FR-011 | NFR-001]
#[test]
fn spacy_model_comes_from_the_spacy_release_with_the_pinned_sha256() {
    let pkgs = lock_packages();
    let p = find_pkg(&pkgs, "en-core-web-sm");
    let prefix = "https://github.com/explosion/spacy-models/releases/download/en_core_web_sm-3.8.0/";
    assert!(p.source.contains(prefix) || p.artifacts.iter().any(|a| a.contains(prefix)), "{} {:?}", p.source, p.artifacts);
    assert!(
        p.artifacts.iter().any(|a| a.contains(prefix) && a.contains("hash = \"sha256:1932429db727d4bff3deed6b34cfc05df17794f4a52eeb26cf8928f7c1a0fb85\"")),
        "{:?}",
        p.artifacts
    );
}

// [Scenario: Pins recorded (FR-011) | docs/requirements FR-011 | FR-011 | NFR-001 | BR-003]
#[test]
fn every_sdist_and_wheel_entry_has_a_sha256_hash_and_every_package_has_a_hashed_artifact() {
    let pkgs = lock_packages();
    assert!(pkgs.len() > 100, "the locked set has well over 100 packages, found {}", pkgs.len());
    for p in &pkgs {
        assert!(!p.name.is_empty() && !p.version.is_empty() && !p.source.is_empty(), "complete block for {:?}", p.name);
        for a in &p.artifacts {
            assert!(has_sha256_hash(a), "{}: artifact without a sha256 hash: {a}", p.name);
        }
        if p.source.contains("virtual") {
            assert_eq!(p.name, "narrate-worker", "virtual sources are only for narrate-worker");
        } else {
            assert!(!p.artifacts.is_empty(), "{} needs at least one hashed artifact", p.name);
        }
    }
}

// [Scenario: Pins recorded (FR-011) | spec-narrate-setup Source allow-list | FR-011 | BR-003 | UR-010]
#[test]
fn every_url_in_the_lock_uses_an_allowed_https_prefix() {
    let text = read_repo_text("python/uv.lock");
    let allowed = [
        "https://pypi.org/simple",
        "https://files.pythonhosted.org/packages/",
        "https://download.pytorch.org/whl/",
        "https://github.com/explosion/spacy-models/releases/download/en_core_web_sm-3.8.0/",
    ];
    for (n, line) in text.lines().enumerate() {
        for scheme in ["https://", "http://", "git+", "ssh://"] {
            let mut from = 0;
            while let Some(pos) = line[from..].find(scheme) {
                let start = from + pos;
                let end = line[start..].find('"').map(|e| e + start).unwrap_or(line.len());
                let url = &line[start..end];
                assert!(
                    allowed.iter().any(|a| url.starts_with(a)),
                    "uv.lock line {}: URL outside the allow-list: {url}",
                    n + 1
                );
                from = end.max(start + scheme.len());
            }
        }
    }
    for p in lock_packages() {
        let s = &p.source;
        assert!(
            !(s.contains("git =") || s.contains("path =") || s.contains("directory =") || s.contains("editable =")),
            "{} uses a disallowed source kind: {s}",
            p.name
        );
    }
}

// ---------------------------------------------------------------------------------------
// pyproject.toml, line endings
// ---------------------------------------------------------------------------------------

// [Scenario: Pins recorded (FR-011) | spec-narrate-test-strategy Repository inspection | FR-011 | BR-003]
#[test]
fn pyproject_declares_the_explicit_pytorch_cu124_index() {
    let text = read_repo_text("python/pyproject.toml");
    let lines: Vec<String> = text.lines().map(|l| l.trim().replace(' ', "")).collect();
    let at = lines
        .iter()
        .position(|l| l == "name=\"pytorch-cu124\"")
        .expect("python/pyproject.toml must declare the pytorch-cu124 index");
    // The index table is the run of lines around `name = ...` between two table headers.
    let mut start = at;
    while start > 0 && !lines[start].starts_with('[') {
        start -= 1;
    }
    let mut end = at + 1;
    while end < lines.len() && !lines[end].starts_with('[') {
        end += 1;
    }
    assert!(
        lines[start..end].iter().any(|l| l == "explicit=true"),
        "the pytorch-cu124 index must set explicit = true: {:?}",
        &lines[start..end]
    );
}

// [Scenario: Line endings fixed | spec-narrate-architecture Pinned inputs | FR-017]
#[test]
fn pinned_inputs_contain_no_carriage_return() {
    for rel in ["crates/narrate/pins.json", "python/pyproject.toml", "python/uv.lock", "python/narrate_worker.py"] {
        let bytes = fs::read(repo_file(rel)).unwrap_or_else(|e| panic!("{rel} must exist: {e}"));
        assert!(!bytes.contains(&b'\r'), "{rel} must not contain a carriage return byte");
        assert!(!bytes.is_empty(), "{rel} must not be empty");
    }
}

// [Requirement: Pinned inputs have stable line endings | spec-narrate-architecture | FR-017]
#[test]
fn gitattributes_marks_the_four_pinned_inputs_as_text_eol_lf() {
    let text = read_repo_text(".gitattributes");
    for rel in ["crates/narrate/pins.json", "python/pyproject.toml", "python/uv.lock", "python/narrate_worker.py"] {
        assert!(
            text.lines().any(|l| l.contains(rel) && l.contains("text") && l.contains("eol=lf")),
            ".gitattributes must mark {rel} as `text eol=lf`"
        );
    }
}

// ---------------------------------------------------------------------------------------
// Workspace and crate manifests (spec-narrate-architecture.md, test-strategy dev-dependencies)
// ---------------------------------------------------------------------------------------

fn dep_line<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.lines().map(|l| l.trim()).find(|l| {
        let c = l.replace(' ', "");
        c.starts_with(&format!("{name}=")) || c.starts_with(&format!("{name}.")) || c.starts_with(&format!("\"{name}\"="))
    })
}

fn section(text: &str, header: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for l in text.lines() {
        let t = l.trim();
        if t.starts_with('[') {
            inside = t == header;
            continue;
        }
        if inside {
            out.push_str(l);
            out.push('\n');
        }
    }
    out
}

// [Scenario: Workspace builds | spec-narrate-architecture Workspace manifest | FR-011 | NFR-001]
#[test]
fn workspace_manifest_uses_the_glob_members_edition_2024_and_exact_dependency_versions() {
    let text = read_repo_text("Cargo.toml");
    let compact = compact_text(&text);
    assert!(compact.contains("members=[\"crates/*\"]"), "members = [\"crates/*\"]");
    assert!(compact.contains("resolver=\"3\""));
    assert!(compact.contains("edition=\"2024\""));
    assert!(compact.contains("rust-version=\"1.99\""));
    assert!(compact.contains("version=\"0.1.0\""));

    let deps = section(&text, "[workspace.dependencies]");
    for (name, version) in [
        ("anyhow", "1.0.104"),
        ("thiserror", "2.0.21"),
        ("clap", "4.6.7"),
        ("serde", "1.0.229"),
        ("serde_json", "1.0.154"),
        ("pulldown-cmark", "0.13.4"),
        ("hound", "3.5.1"),
        ("dirs", "7.0.0"),
        ("sha2", "0.11.0"),
        ("ctrlc", "3.5.2"),
        ("assert_cmd", "2.2.2"),
        ("predicates", "3.1.4"),
        ("tempfile", "3.27.0"),
        ("windows-sys", "0.61.2"),
    ] {
        let line = dep_line(&deps, name).unwrap_or_else(|| panic!("[workspace.dependencies] must declare {name}"));
        assert!(line.contains(&format!("\"={version}\"")), "{name} must be pinned with `=`: {line}");
    }
    let ws = dep_line(&deps, "windows-sys").unwrap().replace(' ', "");
    for feature in ["Win32_Foundation", "Win32_Security", "Win32_System_JobObjects", "Win32_System_Threading", "Win32_System_Console"] {
        assert!(ws.contains(&format!("\"{feature}\"")), "windows-sys feature {feature}: {ws}");
    }
    assert!(dep_line(&deps, "clap").unwrap().contains("derive"));
    assert!(dep_line(&deps, "serde").unwrap().contains("derive"));
    assert!(dep_line(&deps, "pulldown-cmark").unwrap().replace(' ', "").contains("default-features=false"));
}

// [Requirement: Crate manifests + Test command and dev-dependencies | spec-narrate-architecture / spec-narrate-test-strategy | FR-024]
#[test]
fn narrate_manifest_lists_the_test_hooks_feature_and_the_five_dev_dependencies() {
    let text = fs::read_to_string(manifest_dir().join("Cargo.toml")).expect("crates/narrate/Cargo.toml");
    let compact = compact_text(&text);
    assert!(compact.contains("name=\"narrate\""));
    let features = section(&text, "[features]");
    assert!(features.contains("test-hooks"), "feature test-hooks");
    let default_line = features.lines().map(|l| l.replace(' ', "")).find(|l| l.starts_with("default="));
    if let Some(d) = default_line {
        assert!(!d.contains("test-hooks"), "test-hooks must be off by default: {d}");
    }
    assert!(text.contains("narrate-core"), "depends on narrate-core");
    let dev = section(&text, "[dev-dependencies]");
    for name in ["assert_cmd", "predicates", "tempfile", "hound", "serde_json"] {
        let line = dep_line(&dev, name).unwrap_or_else(|| panic!("[dev-dependencies] must list {name}"));
        assert!(line.replace(' ', "").contains("workspace=true"), "{name} uses workspace = true: {line}");
    }
}

// [Requirement: Crate manifests | spec-narrate-architecture | FR-024]
#[test]
fn narrate_core_manifest_lists_tempfile_as_a_dev_dependency() {
    let text = read_repo_text("crates/narrate-core/Cargo.toml");
    let compact = compact_text(&text);
    assert!(compact.contains("name=\"narrate-core\""));
    let dev = section(&text, "[dev-dependencies]");
    let line = dep_line(&dev, "tempfile").expect("[dev-dependencies] must list tempfile");
    assert!(line.replace(' ', "").contains("workspace=true"), "{line}");
    assert!(!compact.contains("[[bin]]"), "narrate-core has no binaries");
}

// [Requirement: Test kit | spec-narrate-test-strategy | FR-005]
#[test]
fn testkit_manifest_is_unpublished_depends_only_on_workspace_dependencies_and_builds_four_binaries() {
    let text = read_repo_text("crates/narrate-testkit/Cargo.toml");
    let compact = compact_text(&text);
    assert!(compact.contains("publish=false"), "publish = false");
    let deps = section(&text, "[dependencies]");
    let names: Vec<String> = deps
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split('=').next())
        .map(|n| n.trim().trim_matches('"').split('.').next().unwrap_or("").to_string())
        .collect();
    for n in &names {
        assert!(["serde", "serde_json", "windows-sys"].contains(&n.as_str()), "unexpected dependency {n}");
    }
    for want in ["serde", "serde_json", "windows-sys"] {
        let line = dep_line(&deps, want).unwrap_or_else(|| panic!("narrate-testkit must depend on {want}"));
        assert!(line.replace(' ', "").contains("workspace=true"), "{want} must be a workspace dependency: {line}");
    }
    for bin in ["narrate-test-worker", "narrate-test-uv", "narrate-test-proxy", "narrate-test-signal"] {
        let declared = text.contains(&format!("\"{bin}\""));
        let by_file = repo_file(&format!("crates/narrate-testkit/src/bin/{bin}.rs")).is_file();
        assert!(declared || by_file, "binary {bin} must be declared or live at src/bin/{bin}.rs");
    }
    assert!(repo_file("crates/narrate-testkit/tests/smoke.rs").is_file());
}

// ---------------------------------------------------------------------------------------
// QA assets (spec-narrate-test-strategy.md, Requirement: QA assets)
// ---------------------------------------------------------------------------------------

const PITCH_PARAGRAPH: &str = "Every evening, the old lighthouse keeper climbed the spiral stairs to light the lamp. From the top, he could see the whole bay, the small village, and the hills beyond. He wrote down the weather in a worn notebook, as his father had done before him. On clear nights, the stars seemed close enough to touch.";

const QA_ASSETS: [&str; 8] = [
    "qa/pitch/pitch-script.txt",
    "qa/pitch/median_f0.py",
    "qa/watermark/check_watermark.py",
    "qa/long/script-10000-words.txt",
    "qa/nfr002/snapshot.ps1",
    "qa/offline/watch_connections.ps1",
    "qa/vram/sample_vram.ps1",
    "qa/installer/check_installer.ps1",
];

fn compact(rel: &str) -> String {
    compact_text(&read_repo_text(rel))
}

// [Scenario: Pitch script fixed | spec-narrate-test-strategy | NFR-004]
#[test]
fn pitch_script_matches_the_spec_text_exactly_and_has_56_words() {
    let bytes = fs::read(repo_file("qa/pitch/pitch-script.txt")).expect("qa/pitch/pitch-script.txt must exist");
    let expected = format!("{PITCH_PARAGRAPH}\n");
    assert_eq!(String::from_utf8(bytes).expect("UTF-8"), expected, "the paragraph followed by one newline");
    assert_eq!(PITCH_PARAGRAPH.split_whitespace().count(), 56);
}

// [Requirement: QA assets (long script) | spec-narrate-test-strategy | NFR-005]
#[test]
fn long_script_is_the_pitch_paragraph_repeated_179_times_separated_by_blank_lines() {
    let text = read_repo_text("qa/long/script-10000-words.txt").replace("\r\n", "\n");
    let parts: Vec<&str> = text.trim_end().split("\n\n").collect();
    assert_eq!(parts.len(), 179);
    for (i, part) in parts.iter().enumerate() {
        assert_eq!(part.trim(), PITCH_PARAGRAPH, "paragraph {}", i + 1);
    }
    assert_eq!(text.split_whitespace().count(), 10_024);
}

// [Requirement: QA assets | spec-narrate-test-strategy | NFR-002 | NFR-004 | NFR-005 | FR-018]
#[test]
fn every_qa_asset_exists_and_is_not_empty() {
    for rel in QA_ASSETS {
        let bytes = fs::read(repo_file(rel)).unwrap_or_else(|e| panic!("{rel} must exist: {e}"));
        assert!(!bytes.is_empty(), "{rel} must not be empty");
    }
}

// [Requirement: QA assets (median_f0.py) | spec-narrate-test-strategy | NFR-004]
#[test]
fn median_f0_script_follows_the_estimator_and_output_contract() {
    let c = compact("qa/pitch/median_f0.py");
    for needle in [
        "librosa.load(",
        "sr=16000",
        "mono=True",
        "librosa.pyin(",
        "fmin=65.0",
        "fmax=500.0",
        "frame_length=1024",
        "voiced_flag",
        "median(",
        "median_f0_hz=",
        "voiced_frames=",
        "frames=",
        "repr(",
        "100",
    ] {
        assert!(c.contains(needle), "median_f0.py must contain `{needle}`");
    }
    assert!(c.contains("exit(2)") || c.contains("exit_code=2") || c.contains("return2"), "exit 2 below 100 voiced frames");
}

// [Requirement: QA assets (check_watermark.py) | spec-narrate-test-strategy | FR-018]
#[test]
fn watermark_check_script_calls_the_perth_extractor_on_the_native_rate() {
    let c = compact("qa/watermark/check_watermark.py");
    for needle in ["librosa.load(", "sr=None", "PerthImplicitWatermarker()", "get_watermark(", "sample_rate=", "watermark="] {
        assert!(c.contains(needle), "check_watermark.py must contain `{needle}`");
    }
}

// [Requirement: QA assets (snapshot.ps1) | spec-narrate-test-strategy | NFR-002]
#[test]
fn snapshot_script_covers_the_locations_registry_path_and_espeak_leftovers() {
    let c = compact("qa/nfr002/snapshot.ps1");
    for needle in [
        "-Out",
        "-Compare",
        "LOCALAPPDATA",
        "APPDATA",
        "USERPROFILE",
        ".local",
        "huggingface",
        "torch",
        "HKCU:",
        "Python",
        "espeak",
        "TEMP",
        "exit1",
    ] {
        assert!(c.contains(needle), "snapshot.ps1 must contain `{needle}`");
    }
    assert!(c.contains("Path"), "the user Path value is recorded");
}

// [Requirement: QA assets (watch_connections.ps1, sample_vram.ps1) | spec-narrate-test-strategy | FR-012 | NFR-005]
#[test]
fn connection_and_vram_scripts_use_the_documented_tools_and_intervals() {
    let w = compact("qa/offline/watch_connections.ps1");
    for needle in ["-ProcessId", "-Out", "Get-NetTCPConnection", "-OwningProcess", "500", "127.0.0.1", "::1", "0.0.0.0", "exit1"] {
        assert!(w.contains(needle), "watch_connections.ps1 must contain `{needle}`");
    }
    let v = compact("qa/vram/sample_vram.ps1");
    for needle in ["-Out", "nvidia-smi", "--query-gpu=memory.used", "--format=csv,noheader,nounits", "-lms200"] {
        assert!(v.contains(needle), "sample_vram.ps1 must contain `{needle}`");
    }
}

// [Requirement: Installer checks | spec-narrate-test-strategy / spec-narrate-installer | FR-016 | NFR-003 | NFR-009]
#[test]
fn installer_check_script_prints_pass_or_fail_per_check() {
    let c = compact("qa/installer/check_installer.ps1");
    for needle in ["PASS", "FAIL", "build.ps1", "unins000", "/VERYSILENT", "/DIR=", "narrate0.1.0"] {
        assert!(c.contains(needle), "check_installer.ps1 must contain `{needle}`");
    }
}

// [Requirement: QA assets (interrupt item 10) | spec-narrate-test-strategy | FR-005]
#[test]
fn qa_instructions_name_spawn_group_for_scripted_interrupt_runs() {
    // PowerShell Start-Process cannot set CREATE_NEW_PROCESS_GROUP, so the scripted interrupt
    // procedure must start narrate through `narrate-test-signal spawn-group`.
    let mentions = QA_ASSETS.iter().filter(|rel| read_repo_text(rel).contains("spawn-group")).count();
    assert!(
        mentions >= 1,
        "at least one QA asset must tell QA to use `narrate-test-signal spawn-group <exe> [args...]`"
    );
}
