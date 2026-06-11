//! Pure bundle-assembly logic: newline normalization, fence sizing, and the
//! export format from the PRD. No I/O here.

/// Output newline style. The PRD's "Platform Default" project setting
/// (Phase 2) resolves to one of these before reaching this module.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Newline {
    Unix,
    Windows,
}

impl Newline {
    fn as_str(self) -> &'static str {
        match self {
            Newline::Unix => "\n",
            Newline::Windows => "\r\n",
        }
    }
}

/// Normalize all newlines (CRLF, lone CR, LF) to the requested style.
pub fn normalize_newlines(s: &str, nl: Newline) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                chars.next_if_eq(&'\n');
                out.push_str(nl.as_str());
            }
            '\n' => out.push_str(nl.as_str()),
            _ => out.push(c),
        }
    }
    out
}

/// A code fence long enough that the content cannot terminate it early:
/// one more backtick than the content's longest run, minimum three.
pub fn fence_for(content: &str) -> String {
    let longest = content
        .as_bytes()
        .split(|&b| b != b'`')
        .map(<[u8]>::len)
        .max()
        .unwrap_or(0);
    "`".repeat((longest + 1).max(3))
}

/// One file's worth of input to bundle assembly, already read and presented.
pub struct BundleFile {
    /// Path as presented per the project's Path Presentation setting
    /// (bare basenames in Phase 1).
    pub display: String,
    pub content: String,
}

/// Assemble the export per the PRD format. `title` falls back upstream
/// (project title, else .bmd/output basename). An empty introduction leaves
/// a blank line in its place. With `toc_links`, TOC entries link to the file
/// headings via GitHub-style anchors.
pub fn assemble(
    title: &str,
    introduction: &str,
    files: &[BundleFile],
    nl: Newline,
    toc_links: bool,
) -> String {
    let n = nl.as_str();
    let mut out = String::new();
    let push_line = |out: &mut String, line: &str| {
        out.push_str(line);
        out.push_str(n);
    };

    // Anchors are unique per document, so feed every heading in order.
    let mut slugger = crate::anchors::Slugger::new();
    slugger.slug(title);
    slugger.slug("Table of Contents");
    let file_headings: Vec<String> = files
        .iter()
        .enumerate()
        .map(|(i, f)| format!("File {}: {}", i + 1, f.display))
        .collect();
    let file_anchors: Vec<String> = file_headings.iter().map(|h| slugger.slug(h)).collect();

    push_line(&mut out, &format!("# {}", title));
    push_line(&mut out, "");
    if !introduction.is_empty() {
        let intro = normalize_newlines(introduction, nl);
        out.push_str(&intro);
        if !intro.ends_with(n) {
            out.push_str(n);
        }
    }
    push_line(&mut out, "");
    push_line(&mut out, "## Table of Contents");
    push_line(&mut out, "");
    for (file, anchor) in files.iter().zip(&file_anchors) {
        if toc_links {
            push_line(&mut out, &format!("- [{}](#{})", file.display, anchor));
        } else {
            push_line(&mut out, &format!("- {}", file.display));
        }
    }

    for (file, heading) in files.iter().zip(&file_headings) {
        let fence = fence_for(&file.content);
        push_line(&mut out, "");
        push_line(&mut out, &format!("## {}", heading));
        push_line(&mut out, "");
        push_line(&mut out, &fence);
        let content = normalize_newlines(&file.content, nl);
        out.push_str(&content);
        if !content.is_empty() && !content.ends_with(n) {
            out.push_str(n);
        }
        push_line(&mut out, &fence);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_mixed_to_unix() {
        assert_eq!(
            normalize_newlines("a\r\nb\rc\nd", Newline::Unix),
            "a\nb\nc\nd"
        );
    }

    #[test]
    fn normalize_mixed_to_windows() {
        assert_eq!(
            normalize_newlines("a\r\nb\rc\nd", Newline::Windows),
            "a\r\nb\r\nc\r\nd"
        );
    }

    #[test]
    fn fence_minimum_is_three() {
        assert_eq!(fence_for("no backticks"), "```");
        assert_eq!(fence_for("inline `code` here"), "```");
        assert_eq!(fence_for("``"), "```");
    }

    #[test]
    fn fence_exceeds_longest_run() {
        assert_eq!(fence_for("a ``` b"), "````");
        assert_eq!(fence_for("x `````` y"), "```````");
    }

    #[test]
    fn fence_handles_run_at_ends() {
        assert_eq!(fence_for("````start"), "`````");
        assert_eq!(fence_for("end````"), "`````");
    }

    fn files() -> Vec<BundleFile> {
        vec![
            BundleFile {
                display: "a.txt".into(),
                content: "alpha\n".into(),
            },
            BundleFile {
                display: "b.md".into(),
                content: "has ``` fence\r\nand crlf".into(),
            },
        ]
    }

    #[test]
    fn assemble_basic_structure() {
        let out = assemble("My Title", "", &files(), Newline::Unix, false);
        let expected = "\
# My Title

\n## Table of Contents

- a.txt
- b.md

## File 1: a.txt

```
alpha
```

## File 2: b.md

````
has ``` fence
and crlf
````
";
        assert_eq!(out, expected);
    }

    #[test]
    fn assemble_includes_introduction() {
        let out = assemble("T", "Hello intro.", &files(), Newline::Unix, false);
        assert!(out.starts_with("# T\n\nHello intro.\n\n## Table of Contents\n"));
    }

    #[test]
    fn assemble_windows_newlines_throughout() {
        let out = assemble("T", "", &files(), Newline::Windows, false);
        assert!(out.contains("## File 2: b.md\r\n"));
        assert!(out.contains("has ``` fence\r\nand crlf\r\n"));
        assert!(!normalize_newlines(&out, Newline::Unix).contains('\r'));
    }

    #[test]
    fn assemble_toc_links_use_github_anchors() {
        let f = vec![
            BundleFile {
                display: "src/config.json".into(),
                content: "a".into(),
            },
            BundleFile {
                display: "lib/config.json".into(),
                content: "b".into(),
            },
        ];
        let out = assemble("T", "", &f, Newline::Unix, true);
        assert!(out.contains("- [src/config.json](#file-1-srcconfigjson)\n"));
        assert!(out.contains("- [lib/config.json](#file-2-libconfigjson)\n"));
        assert!(out.contains("## File 1: src/config.json\n"));
    }

    #[test]
    fn assemble_adds_trailing_newline_before_closing_fence() {
        let f = vec![BundleFile {
            display: "x".into(),
            content: "no trailing newline".into(),
        }];
        let out = assemble("T", "", &f, Newline::Unix, false);
        assert!(out.contains("no trailing newline\n```\n"));
    }
}
