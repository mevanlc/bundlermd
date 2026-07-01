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
    /// Optional Markdown info string appended to the opening code fence.
    pub fence_tag: Option<String>,
    pub content: String,
    pub include_code_fence: bool,
    pub include_in_toc: bool,
    pub header: BundleHeader,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BundleHeader {
    Filename,
    None,
    Custom(String),
}

impl BundleFile {
    fn heading(&self, index: usize) -> Option<String> {
        match &self.header {
            BundleHeader::Filename => Some(format!("File {}: {}", index + 1, self.display)),
            BundleHeader::None => None,
            BundleHeader::Custom(text) => {
                let text = text.trim();
                if text.is_empty() {
                    Some(format!("File {}: {}", index + 1, self.display))
                } else {
                    Some(text.to_string())
                }
            }
        }
    }

    fn toc_label(&self) -> String {
        match &self.header {
            BundleHeader::Custom(text) if !text.trim().is_empty() => text.trim().to_string(),
            _ => self.display.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LineRange {
    start: usize,
    end: usize,
}

fn completed_lines_written(s: &str, nl: &str) -> usize {
    if s.is_empty() {
        0
    } else {
        s.matches(nl).count() + usize::from(!s.ends_with(nl))
    }
}

fn file_line_ranges(
    description: &str,
    files: &[BundleFile],
    nl: Newline,
    toc_line_count: usize,
) -> Vec<LineRange> {
    let n = nl.as_str();
    let mut next_line = 1;

    next_line += 1; // title
    next_line += 1; // blank after title
    next_line += completed_lines_written(description, n);
    next_line += 1; // blank before TOC
    next_line += 1; // TOC heading
    next_line += 1; // blank after TOC heading
    next_line += toc_line_count;

    files
        .iter()
        .enumerate()
        .map(|(i, file)| {
            next_line += 1; // blank before file section
            let mut start = next_line;
            if file.heading(i).is_some() {
                next_line += 1; // file heading
                next_line += 1; // blank before content/fence
            }

            let end = if file.include_code_fence {
                next_line += 1; // opening fence
                let content = normalize_newlines(&file.content, nl);
                next_line += completed_lines_written(&content, n);
                let closing_fence = next_line;
                next_line += 1; // closing fence
                closing_fence
            } else {
                let content = normalize_newlines(&file.content, nl);
                let content_lines = completed_lines_written(&content, n);
                if file.heading(i).is_none() {
                    start = next_line;
                }
                if content_lines == 0 {
                    start
                } else {
                    next_line += content_lines;
                    next_line - 1
                }
            };

            LineRange { start, end }
        })
        .collect()
}

/// Assemble the export per the PRD format. `title` falls back upstream
/// (project title, else .bmd/output basename). An empty description leaves
/// a blank line in its place. With `toc_links`, TOC entries link to the file
/// headings via GitHub-style anchors.
pub fn assemble(
    title: &str,
    description: &str,
    files: &[BundleFile],
    nl: Newline,
    toc_links: bool,
    include_line_ranges_in_headings: bool,
) -> String {
    let n = nl.as_str();
    let mut out = String::new();
    let push_line = |out: &mut String, line: &str| {
        out.push_str(line);
        out.push_str(n);
    };
    let description = normalize_newlines(description, nl);
    let toc_line_count = files.iter().filter(|file| file.include_in_toc).count();
    let line_ranges = if include_line_ranges_in_headings {
        file_line_ranges(&description, files, nl, toc_line_count)
    } else {
        Vec::new()
    };

    // Anchors are unique per document, so feed every heading in order.
    let mut slugger = crate::anchors::Slugger::new();
    slugger.slug(title);
    slugger.slug("Table of Contents");
    let file_headings: Vec<Option<String>> = files
        .iter()
        .enumerate()
        .map(|(i, f)| {
            f.heading(i).map(|base| {
                if let Some(range) = line_ranges.get(i) {
                    format!("{base} -- (lines {} through {})", range.start, range.end)
                } else {
                    base
                }
            })
        })
        .collect();
    let toc_labels: Vec<Option<String>> = files
        .iter()
        .enumerate()
        .map(|(i, file)| {
            if file.include_in_toc {
                let label = file.toc_label();
                if let Some(range) = line_ranges.get(i) {
                    Some(format!(
                        "{label} -- (lines {} through {})",
                        range.start, range.end
                    ))
                } else {
                    Some(label)
                }
            } else {
                None
            }
        })
        .collect();
    let file_anchors: Vec<Option<String>> = file_headings
        .iter()
        .map(|h| h.as_ref().map(|h| slugger.slug(h)))
        .collect();

    push_line(&mut out, &format!("# {}", title));
    push_line(&mut out, "");
    if !description.is_empty() {
        out.push_str(&description);
        if !description.ends_with(n) {
            out.push_str(n);
        }
    }
    push_line(&mut out, "");
    push_line(&mut out, "## Table of Contents");
    push_line(&mut out, "");
    for (label, anchor) in toc_labels.iter().zip(&file_anchors) {
        if let Some(label) = label {
            if toc_links {
                if let Some(anchor) = anchor {
                    push_line(&mut out, &format!("- [{}](#{})", label, anchor));
                } else {
                    push_line(&mut out, &format!("- {}", label));
                }
            } else {
                push_line(&mut out, &format!("- {}", label));
            }
        }
    }

    for (file, heading) in files.iter().zip(&file_headings) {
        push_line(&mut out, "");
        if let Some(heading) = heading {
            push_line(&mut out, &format!("## {}", heading));
            push_line(&mut out, "");
        }
        let content = normalize_newlines(&file.content, nl);
        if file.include_code_fence {
            let fence = fence_for(&file.content);
            if let Some(tag) = &file.fence_tag {
                push_line(&mut out, &format!("{fence}{tag}"));
            } else {
                push_line(&mut out, &fence);
            }
            out.push_str(&content);
            if !content.is_empty() && !content.ends_with(n) {
                out.push_str(n);
            }
            push_line(&mut out, &fence);
        } else {
            out.push_str(&content);
            if !content.is_empty() && !content.ends_with(n) {
                out.push_str(n);
            }
        }
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

    fn file(display: &str, content: &str) -> BundleFile {
        BundleFile {
            display: display.into(),
            fence_tag: None,
            content: content.into(),
            include_code_fence: true,
            include_in_toc: true,
            header: BundleHeader::Filename,
        }
    }

    fn files() -> Vec<BundleFile> {
        vec![
            file("a.txt", "alpha\n"),
            file("b.md", "has ``` fence\r\nand crlf"),
        ]
    }

    #[test]
    fn assemble_basic_structure() {
        let out = assemble("My Title", "", &files(), Newline::Unix, false, false);
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
    fn assemble_includes_description() {
        let out = assemble(
            "T",
            "Hello description.",
            &files(),
            Newline::Unix,
            false,
            false,
        );
        assert!(out.starts_with("# T\n\nHello description.\n\n## Table of Contents\n"));
    }

    #[test]
    fn assemble_adds_code_fence_tags() {
        let f = vec![BundleFile {
            display: "main.rs".into(),
            fence_tag: Some("rust".into()),
            content: "fn main() {}\n".into(),
            include_code_fence: true,
            include_in_toc: true,
            header: BundleHeader::Filename,
        }];
        let out = assemble("T", "", &f, Newline::Unix, false, false);
        assert!(out.contains("```rust\nfn main() {}\n```\n"));
    }

    #[test]
    fn assemble_windows_newlines_throughout() {
        let out = assemble("T", "", &files(), Newline::Windows, false, false);
        assert!(out.contains("## File 2: b.md\r\n"));
        assert!(out.contains("has ``` fence\r\nand crlf\r\n"));
        assert!(!normalize_newlines(&out, Newline::Unix).contains('\r'));
    }

    #[test]
    fn assemble_toc_links_use_github_anchors() {
        let f = vec![
            BundleFile {
                display: "src/config.json".into(),
                fence_tag: None,
                content: "a".into(),
                include_code_fence: true,
                include_in_toc: true,
                header: BundleHeader::Filename,
            },
            BundleFile {
                display: "lib/config.json".into(),
                fence_tag: None,
                content: "b".into(),
                include_code_fence: true,
                include_in_toc: true,
                header: BundleHeader::Filename,
            },
        ];
        let out = assemble("T", "", &f, Newline::Unix, true, false);
        assert!(out.contains("- [src/config.json](#file-1-srcconfigjson)\n"));
        assert!(out.contains("- [lib/config.json](#file-2-libconfigjson)\n"));
        assert!(out.contains("## File 1: src/config.json\n"));
    }

    #[test]
    fn assemble_adds_trailing_newline_before_closing_fence() {
        let f = vec![BundleFile {
            display: "x".into(),
            fence_tag: None,
            content: "no trailing newline".into(),
            include_code_fence: true,
            include_in_toc: true,
            header: BundleHeader::Filename,
        }];
        let out = assemble("T", "", &f, Newline::Unix, false, false);
        assert!(out.contains("no trailing newline\n```\n"));
    }

    #[test]
    fn assemble_can_include_line_ranges_in_file_headings() {
        let out = assemble("My Title", "", &files(), Newline::Unix, false, true);
        assert!(out.contains("- a.txt -- (lines 9 through 13)\n"));
        assert!(out.contains("- b.md -- (lines 15 through 20)\n"));
        assert!(out.contains("## File 1: a.txt -- (lines 9 through 13)\n"));
        assert!(out.contains("## File 2: b.md -- (lines 15 through 20)\n"));
    }

    #[test]
    fn line_ranges_include_heading_through_closing_fence() {
        let f = vec![BundleFile {
            display: "x".into(),
            fence_tag: None,
            content: "no trailing newline".into(),
            include_code_fence: true,
            include_in_toc: true,
            header: BundleHeader::Filename,
        }];
        let out = assemble("T", "one\ntwo", &f, Newline::Unix, false, true);
        let lines: Vec<_> = out.lines().collect();
        assert_eq!(lines[9], "## File 1: x -- (lines 10 through 14)");
        assert_eq!(lines[13], "```");
    }

    #[test]
    fn assemble_can_omit_code_fence() {
        let mut f = file("plain.txt", "plain text\n");
        f.include_code_fence = false;
        let out = assemble("T", "", &[f], Newline::Unix, false, false);
        assert!(out.contains("## File 1: plain.txt\n\nplain text\n"));
        assert!(!out.contains("```\nplain text\n```"));
    }

    #[test]
    fn assemble_can_omit_toc_entry() {
        let mut f = files();
        f[1].include_in_toc = false;
        let out = assemble("T", "", &f, Newline::Unix, false, false);
        assert!(out.contains("## Table of Contents\n\n- a.txt\n\n## File 1: a.txt"));
        assert!(!out.contains("- b.md\n"));
        assert!(out.contains("## File 2: b.md\n"));
    }

    #[test]
    fn assemble_supports_custom_and_missing_headers() {
        let mut f = vec![file("a.txt", "alpha\n"), file("b.txt", "beta\n")];
        f[0].header = BundleHeader::Custom("Overview".into());
        f[1].header = BundleHeader::None;
        let out = assemble("T", "", &f, Newline::Unix, true, false);
        assert!(out.contains("- [Overview](#overview)\n"));
        assert!(out.contains("- b.txt\n"));
        assert!(out.contains("## Overview\n\n```\nalpha\n```\n"));
        assert!(out.contains("```\nbeta\n```\n"));
        assert!(!out.contains("## File 2: b.txt"));
    }
}
