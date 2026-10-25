use pulldown_cmark::{html, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use std::sync::OnceLock;

fn syntax_set() -> &'static syntect::parsing::SyntaxSet {
    static SS: OnceLock<syntect::parsing::SyntaxSet> = OnceLock::new();
    SS.get_or_init(syntect::parsing::SyntaxSet::load_defaults_newlines)
}

fn theme() -> &'static syntect::highlighting::Theme {
    static TH: OnceLock<syntect::highlighting::Theme> = OnceLock::new();
    TH.get_or_init(|| {
        let mut ts = syntect::highlighting::ThemeSet::load_defaults();
        ts.themes.remove("base16-eighties.dark").unwrap_or_else(|| {
            ts.themes
                .into_values()
                .next()
                .expect("syntect ships with built-in themes")
        })
    })
}

fn highlight_code(code: &str, lang: &str) -> String {
    let ss = syntax_set();
    let syntax = ss
        .find_syntax_by_token(lang)
        .or_else(|| ss.find_syntax_by_extension(lang))
        .unwrap_or_else(|| ss.find_syntax_plain_text());
    let _ = syntect::html::highlighted_html_for_string(code, ss, syntax, theme());
    syntect::html::highlighted_html_for_string(code, ss, syntax, theme()).unwrap_or_else(|_| {
        // Fallback: escaped plain block
        format!("<pre><code>{}</code></pre>", escape_html(code))
    })
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn render_markdown_to_html(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(markdown, options);

    // Walk events to inject syntax-highlighted code blocks
    let mut html_output = String::new();
    let mut in_code_block: Option<String> = None;
    let mut code_buf = String::new();

    for event in parser {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(lang) => {
                        let l = lang.trim().to_string();
                        if l.is_empty() {
                            "text".to_string()
                        } else {
                            l
                        }
                    }
                    CodeBlockKind::Indented => "text".to_string(),
                };
                in_code_block = Some(lang);
                code_buf.clear();
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(lang) = in_code_block.take() {
                    if lang.eq_ignore_ascii_case("mermaid") {
                        // Rendered client-side; keep as raw code block for the frontend
                        html_output.push_str(&format!(
                            "<pre class=\"code-block mermaid-block\"><code>{}</code></pre>",
                            escape_html(&code_buf)
                        ));
                    } else {
                        html_output.push_str(&highlight_code(&code_buf, &lang));
                    }
                    code_buf.clear();
                }
            }
            Event::Text(text) => {
                if in_code_block.is_some() {
                    code_buf.push_str(&text);
                } else {
                    html::push_html(&mut html_output, std::iter::once(Event::Text(text)));
                }
            }
            other => {
                html::push_html(&mut html_output, std::iter::once(other));
            }
        }
    }

    // Sanitize rendered HTML against XSS
    sanitize_html(&html_output)
}

/// Render a code note's content (language + raw code) to highlighted HTML.
pub fn render_code_to_html(code: &str, language: &str) -> String {
    highlight_code(code, language)
}

/// Markdown → plain text (used for PDF export and snippets).
pub fn render_markdown_to_text(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(markdown, options);
    let mut out = String::new();
    for event in parser {
        match event {
            Event::Text(t) => out.push_str(&t),
            Event::Code(t) => out.push_str(&t),
            Event::SoftBreak | Event::HardBreak => out.push('\n'),
            Event::Start(Tag::Paragraph | Tag::Heading { .. } | Tag::Item) => {}
            Event::End(TagEnd::Paragraph) => out.push_str("\n\n"),
            Event::End(TagEnd::Heading { .. }) => out.push_str("\n\n"),
            Event::End(TagEnd::Item) => out.push('\n'),
            Event::Rule => out.push_str("\n----\n\n"),
            Event::Start(Tag::CodeBlock(_)) => out.push('\n'),
            Event::End(TagEnd::CodeBlock) => out.push_str("\n\n"),
            _ => {}
        }
    }
    out.trim().to_string()
}

pub fn sanitize_filename(title: &str) -> String {
    let clean: String = title
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();

    let collapsed = clean
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    if collapsed.is_empty() {
        "note".to_string()
    } else {
        collapsed
    }
}

/// Lightweight, deterministic HTML sanitizer to prevent stored XSS attacks
pub fn sanitize_html(html_input: &str) -> String {
    let dangerous_tags = [
        "script", "style", "iframe", "object", "embed", "applet", "form", "svg",
    ];

    let mut sanitized = html_input.to_string();

    for tag in dangerous_tags {
        let open_pat = format!("<{}", tag);
        let close_pat = format!("</{}>", tag);
        sanitized = remove_tag_case_insensitive(&sanitized, &open_pat, &close_pat);
    }

    sanitized = sanitized.replace("javascript:", "blocked:");
    sanitized = sanitized.replace("JAVASCRIPT:", "blocked:");
    sanitized = sanitized.replace("vbscript:", "blocked:");

    strip_event_handlers(&sanitized)
}

fn remove_tag_case_insensitive(input: &str, open_prefix: &str, close_tag: &str) -> String {
    let mut result = String::new();
    let lower = input.to_lowercase();
    let open_prefix_lower = open_prefix.to_lowercase();
    let close_tag_lower = close_tag.to_lowercase();

    let mut i = 0;
    while i < input.len() {
        if lower[i..].starts_with(&open_prefix_lower) {
            if let Some(close_idx) = lower[i..].find(&close_tag_lower) {
                i += close_idx + close_tag_lower.len();
            } else if let Some(tag_end) = lower[i..].find('>') {
                i += tag_end + 1;
            } else {
                break;
            }
        } else {
            let next_char = input[i..].chars().next().unwrap();
            result.push(next_char);
            i += next_char.len_utf8();
        }
    }

    result
}

fn strip_event_handlers(input: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    let mut tag_buf = String::new();

    for c in input.chars() {
        if c == '<' {
            in_tag = true;
            tag_buf.clear();
            tag_buf.push(c);
        } else if c == '>' && in_tag {
            in_tag = false;
            tag_buf.push(c);
            result.push_str(&clean_tag_attributes(&tag_buf));
            tag_buf.clear();
        } else if in_tag {
            tag_buf.push(c);
        } else {
            result.push(c);
        }
    }

    if !tag_buf.is_empty() {
        result.push_str(&tag_buf);
    }

    result
}

fn clean_tag_attributes(tag: &str) -> String {
    let lower = tag.to_lowercase();
    let dangerous_attrs = [
        "onload",
        "onerror",
        "onclick",
        "onmouseover",
        "onfocus",
        "onblur",
        "onkeydown",
        "onkeyup",
        "onchange",
        "onsubmit",
    ];

    for attr in dangerous_attrs {
        if lower.contains(attr) {
            return tag.replace(attr, "blocked-attr");
        }
    }

    tag.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_markdown_basic() {
        let md = "# Title\n\nThis is **bold** and *italic*.\n\n- Item 1\n- Item 2";
        let html = render_markdown_to_html(md);
        assert!(html.contains("<h1>Title</h1>"));
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<li>Item 1</li>"));
    }

    #[test]
    fn test_code_block_highlighted() {
        let md = "```rust\nfn main() { println!(\"hi\"); }\n```";
        let html = render_markdown_to_html(md);
        assert!(html.contains("<pre"));
        assert!(html.contains("main"));
    }

    #[test]
    fn test_xss_script_injection_stripped() {
        let md = "<script>alert('xss')</script>Hello";
        let html = render_markdown_to_html(md);
        assert!(!html.contains("<script"));
        assert!(!html.contains("alert"));
    }

    #[test]
    fn test_xss_event_handler_stripped() {
        let md = "<img src=\"x\" onerror=\"alert(1)\">";
        let html = render_markdown_to_html(md);
        assert!(!html.contains("onerror"));
    }

    #[test]
    fn test_xss_javascript_protocol_blocked() {
        let md = "[Click me](javascript:alert('xss'))";
        let html = render_markdown_to_html(md);
        assert!(!html.contains("javascript:"));
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(
            sanitize_filename("Meeting Notes / Oct 2026"),
            "Meeting-Notes-Oct-2026"
        );
        assert_eq!(sanitize_filename("../../../etc/passwd"), "etc-passwd");
        assert_eq!(sanitize_filename("   "), "note");
    }
}
