//! GitHub-style heading anchors: lowercase, drop punctuation, spaces to
//! hyphens, `-N` suffixes for duplicates. Matches gfm's auto-anchor rules so
//! TOC links resolve when the bundle is viewed on GitHub.

use std::collections::HashMap;

#[derive(Default)]
pub struct Slugger {
    seen: HashMap<String, u32>,
}

impl Slugger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Slug for a heading, unique across all headings fed to this Slugger in
    /// document order.
    pub fn slug(&mut self, heading: &str) -> String {
        let base: String = heading
            .to_lowercase()
            .chars()
            .filter_map(|c| match c {
                ' ' => Some('-'),
                '-' | '_' => Some(c),
                c if c.is_alphanumeric() => Some(c),
                _ => None,
            })
            .collect();
        let count = self.seen.entry(base.clone()).or_insert(0);
        let slug = if *count == 0 {
            base.clone()
        } else {
            format!("{base}-{count}")
        };
        *count += 1;
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_slugs() {
        let mut s = Slugger::new();
        assert_eq!(s.slug("Table of Contents"), "table-of-contents");
        assert_eq!(s.slug("File 1: src/config.json"), "file-1-srcconfigjson");
        assert_eq!(s.slug("Ünïcode Läuft"), "ünïcode-läuft");
        assert_eq!(s.slug("under_score-dash"), "under_score-dash");
    }

    #[test]
    fn duplicates_get_suffixes() {
        let mut s = Slugger::new();
        assert_eq!(s.slug("config.json"), "configjson");
        assert_eq!(s.slug("config.json"), "configjson-1");
        assert_eq!(s.slug("config.json"), "configjson-2");
    }
}
