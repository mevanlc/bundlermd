//! End-to-end export over the checked-in fixture set — the Phase 1 exit
//! criteria from devdocs/PLAN.md, minus the GUI.

use std::path::{Path, PathBuf};

use bundlermd_lib::project::{PathPresentation, ProjectSettings};
use bundlermd_lib::state::{generate_bundle, Limits};

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
    let (markdown, problems) = generate_bundle(
        &files,
        &ProjectSettings::default(),
        None,
        Path::new("/tmp/My Bundle.md"),
        Limits::default(),
    );

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

/// Phase 2 exit criteria: colliding basenames are disambiguated in headers
/// and TOC links resolve (GitHub anchor rules).
#[test]
fn export_disambiguates_collisions_with_toc_links() {
    let dir = tempfile::tempdir().unwrap();
    let make = |rel: &str, content: &str| {
        let p = dir.path().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
        p
    };
    let files = vec![
        make("alpha/config.json", "{\"a\":1}\n"),
        make("beta/config.json", "{\"b\":2}\n"),
        make("readme.txt", "hello\n"),
    ];
    let settings = ProjectSettings {
        toc_links: true,
        path_presentation: PathPresentation::Smart,
        ..Default::default()
    };
    // No project dir → all files are in the basename-disambiguation pool.
    let (markdown, problems) = generate_bundle(
        &files,
        &settings,
        None,
        Path::new("/tmp/out.md"),
        Limits::default(),
    );

    assert!(problems.is_empty());
    assert!(markdown.contains("- [alpha/config.json](#file-1-alphaconfigjson)\n"));
    assert!(markdown.contains("- [beta/config.json](#file-2-betaconfigjson)\n"));
    assert!(markdown.contains("- [readme.txt](#file-3-readmetxt)\n"));
    assert!(markdown.contains("## File 1: alpha/config.json\n"));
    assert!(markdown.contains("## File 2: beta/config.json\n"));
}

#[test]
fn export_can_include_line_ranges_in_toc_and_headings() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.txt");
    let b = dir.path().join("b.txt");
    std::fs::write(&a, "one\n").unwrap();
    std::fs::write(&b, "two\nthree").unwrap();
    let settings = ProjectSettings {
        include_line_ranges_in_headings: true,
        ..Default::default()
    };
    let (markdown, problems) = generate_bundle(
        &[a, b],
        &settings,
        None,
        Path::new("/tmp/out.md"),
        Limits::default(),
    );

    assert!(problems.is_empty());
    assert!(markdown.contains("- a.txt -- (lines 9 through 13)\n"));
    assert!(markdown.contains("- b.txt -- (lines 15 through 20)\n"));
    assert!(markdown.contains("## File 1: a.txt -- (lines 9 through 13)\n"));
    assert!(markdown.contains("## File 2: b.txt -- (lines 15 through 20)\n"));

    let lines: Vec<_> = markdown.lines().collect();
    assert_eq!(lines[8], "## File 1: a.txt -- (lines 9 through 13)");
    assert_eq!(lines[12], "```");
    assert_eq!(lines[14], "## File 2: b.txt -- (lines 15 through 20)");
    assert_eq!(lines[19], "```");
}

/// Phase 3: size limits enforced at export time (tested with tiny limits;
/// the real defaults are 200 MB / 250 MB).
#[test]
fn export_enforces_size_limits() {
    let dir = tempfile::tempdir().unwrap();
    let make = |name: &str, content: &str| {
        let p = dir.path().join(name);
        std::fs::write(&p, content).unwrap();
        p
    };
    let files = vec![
        make("a.txt", "0123456789\n"),    // 11 bytes — fits
        make("big.txt", &"x".repeat(50)), // 50 bytes — over per-file limit
        make("b.txt", "0123456789\n"),    // 11 bytes — fits (total 22)
        make("c.txt", "0123456789\n"),    // 11 bytes — would make total 33 > 25
    ];
    let limits = Limits {
        max_file_bytes: 40,
        max_total_bytes: 25,
    };
    let (markdown, problems) = generate_bundle(
        &files,
        &ProjectSettings::default(),
        None,
        Path::new("/tmp/out.md"),
        limits,
    );

    let reasons: Vec<_> = problems
        .iter()
        .map(|p| {
            (
                Path::new(&p.path).file_name().unwrap().to_str().unwrap(),
                p.reason.as_str(),
            )
        })
        .collect();
    assert_eq!(
        reasons,
        [
            ("big.txt", "exceeds the maximum file size (40 B)"),
            (
                "c.txt",
                "would push the bundle over the maximum total size (25 B)"
            ),
        ]
    );

    // The survivors made it in; an excluded file's size doesn't count
    // toward the total.
    assert!(markdown.contains("File 1: a.txt"));
    assert!(markdown.contains("File 2: b.txt"));
    assert!(!markdown.contains("big.txt"));
    assert!(!markdown.contains("c.txt"));
}

#[test]
fn export_can_omit_description() {
    let files = vec![fixture("plain.txt")];
    let settings = ProjectSettings {
        description: "Hidden description".into(),
        include_description_in_export: false,
        ..Default::default()
    };
    let (markdown, problems) = generate_bundle(
        &files,
        &settings,
        None,
        Path::new("/tmp/out.md"),
        Limits::default(),
    );

    assert!(problems.is_empty());
    assert!(markdown.starts_with("# out\n\n\n## Table of Contents\n"));
    assert!(!markdown.contains("Hidden description"));
}
