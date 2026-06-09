//! End-to-end export over the checked-in fixture set — the Phase 1 exit
//! criteria from devdocs/PLAN.md, minus the GUI.

use std::path::{Path, PathBuf};

use bundlermd_lib::state::generate_bundle;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn export_over_fixture_set() {
    let files = vec![
        fixture("plain.txt"),
        fixture("crlf.txt"),
        fixture("backticks.md"),
        fixture("utf16le_bom.txt"),
        fixture("binary.bin"),
        fixture("utf16le_nobom.txt"),
        fixture("does_not_exist.txt"),
    ];
    let (markdown, problems) = generate_bundle(&files, Path::new("/tmp/My Bundle.md"));

    // Binary, BOMless UTF-16 (documented limitation), and missing files are
    // problems; everything else made it in.
    let problem_paths: Vec<_> = problems
        .iter()
        .map(|p| Path::new(&p.path).file_name().unwrap().to_str().unwrap())
        .collect();
    assert_eq!(
        problem_paths,
        ["binary.bin", "utf16le_nobom.txt", "does_not_exist.txt"]
    );

    // Title from output stem.
    assert!(markdown.starts_with("# My Bundle\n"));

    // TOC lists exactly the included files, in order.
    assert!(markdown.contains(
        "## Table of Contents\n\n- plain.txt\n- crlf.txt\n- backticks.md\n- utf16le_bom.txt\n"
    ));
    assert!(!markdown.contains("binary.bin"));

    // CRLF content is normalized to Unix.
    assert!(markdown.contains("## File 2: crlf.txt\n\n```\nline one\nline two\n```\n"));
    assert!(!markdown.contains('\r'));

    // The four-backtick run forces a five-backtick fence.
    assert!(markdown.contains(
        "## File 3: backticks.md\n\n`````\nhas a ```` four-backtick run\nand ``` three\n`````\n"
    ));

    // UTF-16 with BOM decoded to text, BOM stripped.
    assert!(markdown.contains("## File 4: utf16le_bom.txt\n\n```\nhi\n```\n"));
}
