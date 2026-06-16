import hljs from "highlight.js/lib/common";
import MarkdownIt from "markdown-it";
import "highlight.js/styles/github.css";

const HIGHLIGHT_LANGUAGE_ALIASES = new Map([
  ["c++", "cpp"],
  ["c#", "csharp"],
  ["f#", "fsharp"],
  ["objective-c", "objectivec"],
  ["objective-c++", "objectivec"],
  ["shell", "bash"],
  ["shellsession", "bash"],
  ["sh", "bash"],
  ["zsh", "bash"],
  ["plain-text", "plaintext"],
  ["text", "plaintext"],
  ["tsx", "typescript"],
]);

function escapeHtml(value: string): string {
  return value.replace(/[&<>"]/g, (ch) => {
    switch (ch) {
      case "&":
        return "&amp;";
      case "<":
        return "&lt;";
      case ">":
        return "&gt;";
      default:
        return "&quot;";
    }
  });
}

function languageClass(language: string): string {
  return language.replace(/[^A-Za-z0-9_-]/g, "-");
}

function resolveHighlightLanguage(lang: string): string | null {
  const lower = lang.trim().toLowerCase();
  if (!lower) return null;
  const compact = lower.replace(/\s+/g, "-");
  const candidates = [
    lower,
    compact,
    HIGHLIGHT_LANGUAGE_ALIASES.get(lower),
    HIGHLIGHT_LANGUAGE_ALIASES.get(compact),
  ].filter((candidate): candidate is string => Boolean(candidate));
  return candidates.find((candidate) => hljs.getLanguage(candidate)) ?? null;
}

const MARKDOWN_RENDERER = new MarkdownIt({
  html: false,
  linkify: true,
  highlight(code, lang) {
    const language = resolveHighlightLanguage(lang);
    if (language) {
      const highlighted = hljs.highlight(code, {
        language,
        ignoreIllegals: true,
      }).value;
      return `<pre><code class="hljs language-${languageClass(language)}">${highlighted}</code></pre>`;
    }

    const fallbackClass = lang
      ? ` class="hljs language-${languageClass(lang.trim().toLowerCase())}"`
      : ' class="hljs"';
    return `<pre><code${fallbackClass}>${escapeHtml(code)}</code></pre>`;
  },
});

export function renderMarkdownPreview(markdown: string): string {
  return MARKDOWN_RENDERER.render(markdown);
}
