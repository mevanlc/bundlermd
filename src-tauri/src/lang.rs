//! Code-fence language tag inference from GitHub Linguist data.

use std::path::Path;

use linguist::{
    detect_language_by_extension, detect_language_by_filename, disambiguate, DetectedLanguage,
};

pub fn fence_tag(path: &Path, content: &str) -> Option<String> {
    detect(path, content).and_then(tag_for)
}

pub fn is_markdown(path: &Path, content: &str) -> bool {
    detect(path, content).is_some_and(|language| language.name == "Markdown")
}

fn detect(path: &Path, content: &str) -> Option<DetectedLanguage> {
    let by_filename = detect_language_by_filename(path).ok()?;
    if by_filename.len() == 1 {
        return by_filename.into_iter().next();
    }

    let disambiguated = disambiguate(path, content).ok()?;
    if let Some(lang) = disambiguated.into_iter().next() {
        return Some(lang);
    }

    let by_extension = detect_language_by_extension(path).ok()?;
    if by_extension.len() == 1 {
        by_extension.into_iter().next()
    } else {
        None
    }
}

fn tag_for(lang: DetectedLanguage) -> Option<String> {
    // Mirrors github-linguist's Language#default_alias, which is prepended to
    // every language's alias list and accepted for fenced code blocks.
    let tag = lang
        .name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_whitespace() { '-' } else { c })
        .collect::<String>();
    if tag.is_empty() {
        None
    } else {
        Some(tag)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{fence_tag, is_markdown};

    #[test]
    fn detects_unambiguous_extensions() {
        assert_eq!(
            fence_tag(Path::new("src/main.rs"), "fn main() {}\n"),
            Some("rust".into())
        );
        assert_eq!(
            fence_tag(
                Path::new("src/App.tsx"),
                "export function App() { return <main />; }\n"
            ),
            Some("tsx".into())
        );
        assert_eq!(
            fence_tag(Path::new("config/settings.json"), "{\"ok\": true}\n"),
            Some("json".into())
        );
        assert_eq!(
            fence_tag(Path::new("src/types.ts"), "type Message = string;\n"),
            Some("typescript".into())
        );
    }

    #[test]
    fn detects_exact_filenames() {
        assert_eq!(
            fence_tag(Path::new("Makefile"), "all:\n\tcargo test\n"),
            Some("makefile".into())
        );
        assert_eq!(
            fence_tag(Path::new(".gitignore"), "target/\nnode_modules/\n"),
            Some("ignore-list".into())
        );
    }

    #[test]
    fn disambiguates_from_content() {
        assert_eq!(
            fence_tag(
                Path::new("include/example.h"),
                "#include <iostream>\nusing namespace std;\n"
            ),
            Some("c++".into())
        );
        assert_eq!(
            fence_tag(
                Path::new("notes.txt"),
                "The quick brown fox jumps over the lazy dog.\n"
            ),
            Some("text".into())
        );
    }

    #[test]
    fn leaves_unknown_extensions_untagged() {
        assert_eq!(fence_tag(Path::new("file.unknownext"), "content"), None);
    }

    #[test]
    fn identifies_markdown_sources() {
        assert!(is_markdown(Path::new("README.md"), "# Read me\n"));
        assert!(!is_markdown(Path::new("notes.txt"), "[link](target)\n"));
    }
}
