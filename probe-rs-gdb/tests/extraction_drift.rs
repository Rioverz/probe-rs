//! Extraction drift check. The expected crate is produced, not written: the test
//! reads upstream's GDB server source at `PINNED_REV` from git, applies the two
//! documented deltas and compares the result with `src/gdb` file by file. A second
//! check binds the whole ported crate to `PORTED_CRATE_SHA256`. Newline-normalized,
//! so it is identical on CRLF and LF checkouts. See EXTRACTION.md for the deltas.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

const PINNED_REV: &str = "48f5e4d53c690a1d40c2454033c6f785b4f4f95c";
// Upstream source directory at PINNED_REV, relative to the repository root.
const UPSTREAM_DIR: &str = "probe-rs-tools/src/bin/probe-rs/cmd/gdb_server";

// gdb_server/mod.rs is authored in this fork (delta #1), not an extracted engine file.
const NOT_EXTRACTED: &[&str] = &["mod.rs"];
const DESC_MOD_REL: &str = "target/desc/mod.rs";
const DESC_TEST_REL: &str = "target/desc/test.rs";

// Documented desc/mod.rs transform (delta #2): drop the trailing test-module block.
const DESC_MOD_MARKER: &str = "\n\n#[cfg(test)]\nmod test;\n";

// SHA-256 over every src/**/*.rs of this crate, see crate_sha256.
const PORTED_CRATE_SHA256: &str =
    "b25920d1a18cbad3a01adf96d06c36cf15ff16e77cd653449b71d99588249f0e";

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_norm(p: &Path) -> String {
    let bytes = fs::read(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
    String::from_utf8(bytes)
        .unwrap_or_else(|e| panic!("utf8 {}: {e}", p.display()))
        .replace("\r\n", "\n")
}

fn sha256_hex(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    hex::encode(hasher.finalize())
}

fn transform_desc_mod(source_lf: &str) -> String {
    let head = source_lf.strip_suffix(DESC_MOD_MARKER).unwrap_or_else(|| {
        panic!("pinned desc/mod.rs no longer ends with the test-module block; re-derive against {PINNED_REV}")
    });
    format!("{head}\n")
}

/// Runs git in this repository; the error is git's own message.
fn git(args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(crate_root().join(".."))
        .args(args)
        .output()
        .map_err(|e| format!("git could not run: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Upstream's `.rs` files under `UPSTREAM_DIR` at `PINNED_REV`, keyed by their
/// path relative to that directory. Fails, never skips, when the commit is absent.
fn source_map() -> BTreeMap<String, String> {
    let commit = format!("{PINNED_REV}^{{commit}}");
    if let Err(e) = git(&["cat-file", "-e", &commit]) {
        panic!(
            "{PINNED_REV} is not in this clone; use a full clone or run git fetch origin {PINNED_REV} ({e})"
        );
    }
    let listed = git(&[
        "ls-tree",
        "-r",
        "--full-tree",
        "--name-only",
        PINNED_REV,
        UPSTREAM_DIR,
    ])
    .unwrap_or_else(|e| panic!("list {UPSTREAM_DIR} at {PINNED_REV}: {e}"));
    let prefix = format!("{UPSTREAM_DIR}/");
    let mut map = BTreeMap::new();
    for path in String::from_utf8(listed).expect("utf8 path list").lines() {
        if !path.ends_with(".rs") {
            continue;
        }
        let blob = git(&["show", &format!("{PINNED_REV}:{path}")])
            .unwrap_or_else(|e| panic!("read {path} at {PINNED_REV}: {e}"));
        let text = String::from_utf8(blob).unwrap_or_else(|e| panic!("utf8 {path}: {e}"));
        let rel = path
            .strip_prefix(&prefix)
            .expect("path under the source directory");
        map.insert(rel.to_string(), text.replace("\r\n", "\n"));
    }
    assert!(
        !map.is_empty(),
        "no .rs source under {UPSTREAM_DIR} at {PINNED_REV}"
    );
    map
}

// Enumerates every `.rs` file under `root`, keyed by its path relative to `base`
// with `/` separators. Snapshots (`.snap`) and other non-Rust files are out of scope.
fn collect_rs(root: &Path, base: &Path, out: &mut BTreeMap<String, String>) {
    for entry in fs::read_dir(root).unwrap_or_else(|e| panic!("read_dir {}: {e}", root.display())) {
        let p = entry.unwrap().path();
        if p.is_dir() {
            collect_rs(&p, base, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            let rel = p
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            out.insert(rel, read_norm(&p));
        }
    }
}

fn extracted_map() -> BTreeMap<String, String> {
    let root = crate_root().join("src/gdb");
    let mut map = BTreeMap::new();
    collect_rs(&root, &root, &mut map);
    map
}

/// Every `src/**/*.rs` of the crate, keyed by its path relative to the crate root.
fn crate_map() -> BTreeMap<String, String> {
    let root = crate_root();
    let mut map = BTreeMap::new();
    collect_rs(&root.join("src"), &root, &mut map);
    map
}

/// SHA-256 over each file's path, a NUL, its normalized text and a NUL, in path order.
fn crate_sha256(files: &BTreeMap<String, String>) -> String {
    let mut hasher = Sha256::new();
    for (rel, text) in files {
        hasher.update(rel.as_bytes());
        hasher.update([0]);
        hasher.update(text.as_bytes());
        hasher.update([0]);
    }
    hex::encode(hasher.finalize())
}

/// The pinned source with the two documented deltas applied.
fn expected_crate(source: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    source
        .iter()
        .filter(|(rel, _)| !NOT_EXTRACTED.contains(&rel.as_str()) && rel.as_str() != DESC_TEST_REL)
        .map(|(rel, text)| {
            let text = if rel == DESC_MOD_REL {
                transform_desc_mod(text)
            } else {
                text.clone()
            };
            (rel.clone(), text)
        })
        .collect()
}

/// Compares the extracted crate with the pinned source plus deltas. Pure over the
/// provided contents so a test can inject drift and assert it is caught.
fn verify(
    source: &BTreeMap<String, String>,
    extracted: &BTreeMap<String, String>,
) -> Result<(), Vec<String>> {
    let expected = expected_crate(source);
    let mut errs = Vec::new();
    for (rel, want) in &expected {
        match extracted.get(rel) {
            None => errs.push(format!("extracted crate missing {rel}")),
            Some(got) if got != want => errs.push(format!(
                "extracted {rel} drifted from pinned {PINNED_REV} (got {})",
                sha256_hex(got)
            )),
            Some(_) => {}
        }
    }
    for rel in extracted.keys() {
        if !NOT_EXTRACTED.contains(&rel.as_str()) && !expected.contains_key(rel) {
            errs.push(format!("unexpected extracted engine file: {rel}"));
        }
    }
    if errs.is_empty() { Ok(()) } else { Err(errs) }
}

#[test]
fn extracted_crate_matches_pinned_source_plus_deltas() {
    if let Err(errs) = verify(&source_map(), &extracted_map()) {
        panic!("extraction drift from {PINNED_REV}:\n{}", errs.join("\n"));
    }
    assert!(
        !crate_root().join("src/gdb/target/desc/snapshots").exists(),
        "desc/snapshots/ must not be extracted"
    );
}

#[test]
fn ported_crate_matches_its_recorded_hash() {
    let got = crate_sha256(&crate_map());
    assert_eq!(
        got, PORTED_CRATE_SHA256,
        "ported crate changed; record the port edit in EXTRACTION.md, then set PORTED_CRATE_SHA256 to {got}"
    );
}

#[test]
fn ported_crate_edit_is_caught() {
    let mut files = crate_map();
    let edited = format!("{}\n// injected drift\n", files["src/gdb/stub.rs"]);
    files.insert("src/gdb/stub.rs".to_string(), edited);
    assert_ne!(crate_sha256(&files), PORTED_CRATE_SHA256);
}

#[test]
fn extracted_only_drift_is_caught() {
    let source = source_map();
    let mut extracted = extracted_map();
    extracted.insert(
        "arch.rs".to_string(),
        format!("{}\n// injected drift\n", extracted["arch.rs"]),
    );

    let errs = verify(&source, &extracted).expect_err("an extracted-only edit must fail");
    assert!(
        errs.iter().any(|e| e.starts_with("extracted arch.rs")),
        "failure must name the extracted drift; got {errs:?}"
    );
}

#[test]
fn extra_source_only_file_is_caught() {
    let mut source = source_map();
    let extracted = extracted_map();
    source.insert(
        "target/new_helper.rs".to_string(),
        "fn helper() {}\n".to_string(),
    );

    let errs = verify(&source, &extracted).expect_err("an extra source-only .rs must fail");
    assert!(
        errs.iter()
            .any(|e| e == "extracted crate missing target/new_helper.rs"),
        "failure must name the extra source path; got {errs:?}"
    );
}
