//! Guard: the GDB library must borrow the worker's session, never open a probe
//! of its own. Fails if any non-comment line in this crate's sources contains a
//! probe enumeration, open, or attach path. This is the cross-platform,
//! self-contained equivalent of the HUB-0A spike's shell/python guard.

use std::fs;
use std::path::Path;

// Second-handle paths the worker-owned seam forbids: `simple_attach` and the
// broader enumerate/open/attach family, including `.open(` / `.attach(`.
const FORBIDDEN: &[&str] = &[
    "simple_attach",
    "auto_attach",
    "attach_under_reset",
    "Lister",
    "list_all",
    "all_probes",
    "open_probe",
    "Probe::open",
    "FakeProbe::open",
    "Session::auto_attach",
    ".attach(",
    ".open(",
];

fn scan_dir(dir: &Path, hits: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("read_dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            scan_dir(&path, hits);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("read source");
        for (i, raw) in text.lines().enumerate() {
            let line = raw.trim_start();
            // Skip line comments so the explanatory prose that names these
            // forbidden APIs (to state they are absent) does not trip the guard.
            if line.starts_with("//") {
                continue;
            }
            for needle in FORBIDDEN {
                if line.contains(needle) {
                    hits.push(format!("{}:{}: {}", path.display(), i + 1, raw.trim()));
                }
            }
        }
    }
}

#[test]
fn no_enumerate_open_or_attach_in_library_sources() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut hits = Vec::new();
    scan_dir(&src, &mut hits);
    assert!(
        hits.is_empty(),
        "independent probe-ownership path found in the GDB library:\n{}",
        hits.join("\n")
    );
}
