//! Source-preserving Markdown transformations applied before bundle assembly.

use std::ops::Range;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

struct LinkSpan {
    whole: Range<usize>,
    label: Option<Range<usize>>,
}

/// Remove parser-recognized Markdown links while preserving their label source,
/// including any emphasis or image markup inside the label. When
/// `remove_labels` is true, remove the complete link instead.
pub fn remove_links(markdown: &str, remove_labels: bool) -> String {
    let mut links = Vec::new();
    let mut active: Option<LinkSpan> = None;

    for (event, range) in Parser::new(markdown).into_offset_iter() {
        match event {
            Event::Start(Tag::Link { .. }) => {
                active = Some(LinkSpan {
                    whole: range,
                    label: None,
                });
            }
            Event::End(TagEnd::Link) => {
                if let Some(link) = active.take() {
                    links.push(link);
                }
            }
            _ => {
                if let Some(link) = &mut active {
                    let start = range.start.max(link.whole.start);
                    let end = range.end.min(link.whole.end);
                    if start < end {
                        link.label = Some(match link.label.take() {
                            Some(label) => label.start.min(start)..label.end.max(end),
                            None => start..end,
                        });
                    }
                }
            }
        }
    }

    if links.is_empty() {
        return markdown.to_string();
    }

    let mut output = String::with_capacity(markdown.len());
    let mut cursor = 0;
    for link in links {
        output.push_str(&markdown[cursor..link.whole.start]);
        if !remove_labels {
            if let Some(label) = link.label {
                output.push_str(&markdown[label]);
            }
        }
        cursor = link.whole.end;
    }
    output.push_str(&markdown[cursor..]);
    output
}

#[cfg(test)]
mod tests {
    use super::remove_links;

    #[test]
    fn removes_inline_reference_and_autolinks() {
        let input = concat!(
            "An [inline](https://example.com \"Example\"), ",
            "a [reference][ref], and <https://example.org>.\n\n",
            "[ref]: https://example.net\n",
        );
        assert_eq!(
            remove_links(input, false),
            concat!(
                "An inline, a reference, and https://example.org.\n\n",
                "[ref]: https://example.net\n",
            ),
        );
    }

    #[test]
    fn preserves_label_markup_images_and_code() {
        let input = concat!(
            "[**bold** and `code`](https://example.com)\n\n",
            "![alt](image.png)\n\n",
            "`[code link](https://example.com)`\n\n",
            "```markdown\n[code block](https://example.com)\n```\n",
        );
        assert_eq!(
            remove_links(input, false),
            concat!(
                "**bold** and `code`\n\n",
                "![alt](image.png)\n\n",
                "`[code link](https://example.com)`\n\n",
                "```markdown\n[code block](https://example.com)\n```\n",
            ),
        );
    }

    #[test]
    fn can_remove_links_and_their_labels() {
        assert_eq!(
            remove_links("Before [first](one) and [second](two) after.", true),
            "Before  and  after.",
        );
    }
}
