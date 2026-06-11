//! Path presentation per the PRD: how file paths are rendered in the TOC and
//! per-file headers. Pure functions, no I/O.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use crate::project::PathPresentation;

/// Render `paths` for display according to the presentation mode. Returns one
/// display string per input path, in order.
pub fn presented_paths(
    paths: &[PathBuf],
    presentation: &PathPresentation,
    project_dir: Option<&Path>,
) -> Vec<String> {
    match presentation {
        PathPresentation::Absolute => paths.iter().map(|p| display(p)).collect(),
        PathPresentation::Fixed { location } => paths
            .iter()
            .map(|p| relative_to(p, Path::new(location)).unwrap_or_else(|| display(p)))
            .collect(),
        PathPresentation::Smart => smart(paths, project_dir),
    }
}

fn display(p: &Path) -> String {
    p.display().to_string()
}

fn segments(p: &Path) -> Vec<String> {
    p.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

/// Last `n` path segments joined with '/'. `n` larger than the segment count
/// yields the whole segment list.
fn suffix(segs: &[String], n: usize) -> String {
    let start = segs.len().saturating_sub(n);
    segs[start..].join("/")
}

/// Relative path from `base` to `path` (lexical; `..` components allowed).
/// `None` when there is no common root (e.g. different Windows drives).
fn relative_to(path: &Path, base: &Path) -> Option<String> {
    let mut path_comps = path.components().peekable();
    let mut base_comps = base.components().peekable();
    // Both must share a root for a lexical relative path to exist.
    while let (Some(p), Some(b)) = (path_comps.peek(), base_comps.peek()) {
        if p == b {
            path_comps.next();
            base_comps.next();
        } else {
            break;
        }
    }
    let ups = base_comps
        .filter(|c| matches!(c, Component::Normal(_)))
        .count();
    let rest: Vec<_> = path_comps.collect();
    if rest.iter().any(|c| !matches!(c, Component::Normal(_))) {
        // Diverged before consuming the roots: no common root.
        return None;
    }
    let mut parts: Vec<String> = std::iter::repeat_n("..".to_string(), ups).collect();
    parts.extend(
        rest.iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    if parts.is_empty() {
        return Some(".".into());
    }
    Some(parts.join("/"))
}

/// Smart Relative per the PRD:
/// - files under the .bmd project's directory are shown relative to it;
/// - all others get a bare basename, except that basename-colliding sets are
///   disambiguated by progressively lengthening their path-segment suffix
///   (deepest dirname segment first) until the set is unambiguous.
fn smart(paths: &[PathBuf], project_dir: Option<&Path>) -> Vec<String> {
    let mut out: Vec<Option<String>> = paths
        .iter()
        .map(|p| {
            project_dir
                .and_then(|d| p.strip_prefix(d).ok())
                .map(|rel| segments(rel).join("/"))
        })
        .collect();

    // Group the remaining (non-project-relative) files by basename.
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, slot) in out.iter().enumerate() {
        if slot.is_none() {
            let segs = segments(&paths[i]);
            let base = segs.last().cloned().unwrap_or_default();
            groups.entry(base).or_default().push(i);
        }
    }

    for indices in groups.values() {
        let seg_lists: Vec<Vec<String>> = indices.iter().map(|&i| segments(&paths[i])).collect();
        let max_len = seg_lists.iter().map(Vec::len).max().unwrap_or(1);
        // Find the shortest uniform suffix length that disambiguates the set.
        let mut chosen = max_len;
        for n in 1..=max_len {
            let mut seen = std::collections::HashSet::new();
            if seg_lists.iter().all(|s| seen.insert(suffix(s, n))) {
                chosen = n;
                break;
            }
        }
        for (&i, segs) in indices.iter().zip(&seg_lists) {
            out[i] = Some(suffix(segs, chosen));
        }
    }

    out.into_iter().map(|s| s.unwrap_or_default()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    fn smart_with(list: &[&str], proj: Option<&str>) -> Vec<String> {
        presented_paths(&paths(list), &PathPresentation::Smart, proj.map(Path::new))
    }

    #[test]
    fn smart_table() {
        // (input paths, project dir, expected)
        let cases: &[(&[&str], Option<&str>, &[&str])] = &[
            // Unique basenames, no project dir: bare basenames.
            (&["/a/one.txt", "/b/two.txt"], None, &["one.txt", "two.txt"]),
            // Files under the project dir are relative to it, even if deep.
            (
                &["/proj/src/main.rs", "/proj/README.md", "/elsewhere/x.txt"],
                Some("/proj"),
                &["src/main.rs", "README.md", "x.txt"],
            ),
            // Basename collision: one extra segment suffices.
            (
                &["/a/x/config.json", "/a/y/config.json"],
                None,
                &["x/config.json", "y/config.json"],
            ),
            // Collision where one extra segment does NOT suffice: deepest
            // segments equal, keep adding.
            (
                &["/one/sub/config.json", "/two/sub/config.json"],
                None,
                &["one/sub/config.json", "two/sub/config.json"],
            ),
            // Mixed: colliding pair disambiguated, unique file untouched.
            (
                &["/a/conf.json", "/b/conf.json", "/c/other.txt"],
                None,
                &["a/conf.json", "b/conf.json", "other.txt"],
            ),
            // Different depths in a colliding set.
            (
                &["/deep/er/conf.json", "/conf.json"],
                None,
                &["er/conf.json", "conf.json"],
            ),
            // Project-relative files do not participate in disambiguation.
            (
                &["/proj/config.json", "/other/config.json"],
                Some("/proj"),
                &["config.json", "config.json"],
            ),
        ];
        for (input, proj, expected) in cases {
            assert_eq!(
                smart_with(input, *proj),
                expected.to_vec(),
                "case: {input:?} proj={proj:?}"
            );
        }
    }

    #[test]
    fn absolute_mode() {
        let got = presented_paths(&paths(&["/a/b.txt"]), &PathPresentation::Absolute, None);
        assert_eq!(got, ["/a/b.txt"]);
    }

    #[test]
    fn fixed_mode_relative_and_updirs() {
        let pres = PathPresentation::Fixed {
            location: "/base/dir".into(),
        };
        let got = presented_paths(
            &paths(&["/base/dir/sub/f.txt", "/base/other/g.txt", "/f2.txt"]),
            &pres,
            None,
        );
        assert_eq!(got, ["sub/f.txt", "../other/g.txt", "../../f2.txt"]);
    }
}
