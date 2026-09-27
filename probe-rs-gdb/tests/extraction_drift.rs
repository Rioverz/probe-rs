//! Extraction drift check (OPI-GDB-LIB): normalized hashes of the in-fork GDB
//! source and the extracted crate are bound to the committed baseline of the
//! pinned probe-rs revision in `tests/pinned_gdb_server.sha256`. Newline-normalized,
//! so it is identical on CRLF and LF checkouts. See EXTRACTION.md for the deltas.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const PINNED_REV: &str = "48f5e4d53c690a1d40c2454033c6f785b4f4f95c";

// gdb_server/mod.rs is OPI-authored (delta #1), not an extracted engine file.
const OPI_AUTHORED: &[&str] = &["mod.rs"];
const DESC_MOD_REL: &str = "target/desc/mod.rs";
const DESC_TEST_REL: &str = "target/desc/test.rs";

// Documented desc/mod.rs transform (delta #2): drop the trailing test-module block.
const DESC_MOD_MARKER: &str = "\n\n#[cfg(test)]\nmod test;\n";
// SHA-256 of the extracted desc/mod.rs = transform(pinned source), from the pin.
const EXPECTED_EXTRACTED_DESC_MOD_SHA256: &str =
    "036bdbe94d4aeb75b6b613174c843603819d9533905cddd9f437a252ba4382b1";

fn upstream_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("probe-rs-tools/src/bin/probe-rs/cmd/gdb_server")
}

fn crate_gdb_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/gdb")
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

fn load_baseline() -> BTreeMap<String, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pinned_gdb_server.sha256");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (hash, rel) = line
            .split_once("  ")
            .unwrap_or_else(|| panic!("malformed baseline line: {line:?}"));
        map.insert(rel.to_string(), hash.to_string());
    }
    assert!(!map.is_empty(), "empty baseline");
    map
}

// Enumerates every `.rs` file under `root`, keyed by its path relative to `root`.
// Snapshots (`.snap`) and other non-Rust files are outside the baseline's scope.
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

fn source_map() -> BTreeMap<String, String> {
    let root = upstream_root();
    let mut map = BTreeMap::new();
    collect_rs(&root, &root, &mut map);
    map
}

fn extracted_map() -> BTreeMap<String, String> {
    let root = crate_gdb_root();
    let mut map = BTreeMap::new();
    collect_rs(&root, &root, &mut map);
    map
}

/// Compares the source and extracted trees against the committed baseline. Pure
/// over the provided contents so a test can inject drift and assert it is caught.
fn verify(
    baseline: &BTreeMap<String, String>,
    source: &BTreeMap<String, String>,
    extracted: &BTreeMap<String, String>,
) -> Result<(), Vec<String>> {
    let mut errs = Vec::new();

    // Source is exactly the pinned `.rs` set, each matching its pinned hash.
    for (rel, want) in baseline {
        match source.get(rel) {
            None => errs.push(format!("source missing pinned file: {rel}")),
            Some(c) => {
                let got = sha256_hex(c);
                if &got != want {
                    errs.push(format!("source {rel} != pinned {PINNED_REV} (got {got})"));
                }
            }
        }
    }
    for rel in source.keys() {
        if !baseline.contains_key(rel) {
            errs.push(format!("source has extra .rs not in baseline: {rel}"));
        }
    }

    // Re-derive the desc/mod.rs transform from the source and confirm it yields the
    // committed extracted hash.
    if let Some(src_desc) = source.get(DESC_MOD_REL) {
        let got = sha256_hex(&transform_desc_mod(src_desc));
        if got != EXPECTED_EXTRACTED_DESC_MOD_SHA256 {
            errs.push(format!(
                "desc/mod.rs transform no longer yields the committed extracted hash (got {got})"
            ));
        }
    }

    // Extracted crate matches the baseline, except the documented deltas.
    for (rel, want) in baseline {
        if OPI_AUTHORED.contains(&rel.as_str()) {
            continue;
        }
        if rel == DESC_TEST_REL {
            if extracted.contains_key(rel) {
                errs.push(format!("{rel} must not be extracted"));
            }
            continue;
        }
        let expected = if rel == DESC_MOD_REL {
            EXPECTED_EXTRACTED_DESC_MOD_SHA256.to_string()
        } else {
            want.clone()
        };
        match extracted.get(rel) {
            None => errs.push(format!("extracted crate missing {rel}")),
            Some(c) => {
                let got = sha256_hex(c);
                if got != expected {
                    errs.push(format!(
                        "extracted {rel} drifted from pinned {PINNED_REV} (got {got})"
                    ));
                }
            }
        }
    }

    // No extra extracted engine file beyond the pinned set.
    for rel in extracted.keys() {
        if OPI_AUTHORED.contains(&rel.as_str()) {
            continue;
        }
        if !baseline.contains_key(rel) || rel == DESC_TEST_REL {
            errs.push(format!("unexpected extracted engine file: {rel}"));
        }
    }

    if errs.is_empty() { Ok(()) } else { Err(errs) }
}

#[test]
fn source_and_extracted_match_pinned_baseline() {
    let baseline = load_baseline();
    let source = source_map();
    let extracted = extracted_map();
    if let Err(errs) = verify(&baseline, &source, &extracted) {
        panic!("extraction drift from {PINNED_REV}:\n{}", errs.join("\n"));
    }
    assert!(
        !crate_gdb_root().join("target/desc/snapshots").exists(),
        "desc/snapshots/ must not be extracted"
    );
}

#[test]
fn simultaneous_source_and_copy_edit_is_caught() {
    let baseline = load_baseline();
    let mut source = source_map();
    let mut extracted = extracted_map();

    let edited = format!("{}\n// injected drift\n", source["stub.rs"]);
    source.insert("stub.rs".to_string(), edited.clone());
    extracted.insert("stub.rs".to_string(), edited);

    let errs = verify(&baseline, &source, &extracted)
        .expect_err("an identical source+copy edit must still fail");
    assert!(
        errs.iter().any(|e| e.starts_with("source stub.rs")),
        "failure must name the source drift; got {errs:?}"
    );
}

#[test]
fn extracted_only_drift_is_caught() {
    let baseline = load_baseline();
    let source = source_map();
    let mut extracted = extracted_map();
    extracted.insert(
        "arch.rs".to_string(),
        format!("{}\n// injected drift\n", extracted["arch.rs"]),
    );

    let errs =
        verify(&baseline, &source, &extracted).expect_err("an extracted-only edit must fail");
    assert!(
        errs.iter().any(|e| e.starts_with("extracted arch.rs")),
        "failure must name the extracted drift; got {errs:?}"
    );
}

#[test]
fn extra_source_only_file_is_caught() {
    let baseline = load_baseline();
    let mut source = source_map();
    let extracted = extracted_map();
    source.insert(
        "target/new_helper.rs".to_string(),
        "fn helper() {}\n".to_string(),
    );

    let errs =
        verify(&baseline, &source, &extracted).expect_err("an extra source-only .rs must fail");
    assert!(
        errs.iter()
            .any(|e| e.contains("extra .rs") && e.contains("target/new_helper.rs")),
        "failure must name the extra source path; got {errs:?}"
    );
}
