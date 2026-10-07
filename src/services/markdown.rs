use pulldown_cmark::{html, Options, Parser};

pub fn render_markdown_to_html(markdown: &str) -> String {
    // Enable tables, footnotes, strikethrough, tasklists
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(markdown, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);

    // Sanitize rendered HTML against XSS
    sanitize_html(&html_output)
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
    // 1. Remove dangerous script, iframe, object, embed, style, form tags
    let dangerous_tags = [
        "script", "style", "iframe", "object", "embed", "applet", "form", "svg",
    ];

    let mut sanitized = html_input.to_string();

    for tag in dangerous_tags {
        let open_pat = format!("<{}", tag);
        let close_pat = format!("</{}>", tag);

        // Case-insensitive removal of open and close tags
        sanitized = remove_tag_case_insensitive(&sanitized, &open_pat, &close_pat);
    }

    // 2. Remove javascript: pseudo-protocols
    sanitized = sanitized.replace("javascript:", "blocked:");
    sanitized = sanitized.replace("JAVASCRIPT:", "blocked:");
    sanitized = sanitized.replace("vbscript:", "blocked:");

    // 3. Strip event handlers (on\w+=...)
    sanitized = strip_event_handlers(&sanitized);

    sanitized
}

fn remove_tag_case_insensitive(input: &str, open_prefix: &str, close_tag: &str) -> String {
    let mut result = String::new();
    let lower = input.to_lowercase();
    let open_prefix_lower = open_prefix.to_lowercase();
    let close_tag_lower = close_tag.to_lowercase();

    let mut i = 0;
    while i < input.len() {
        if lower[i..].starts_with(&open_prefix_lower) {
            // Find end of this tag or corresponding close tag
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
            // Sanitize attributes inside this tag
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
    // Check if tag contains dangerous event handlers like " onerror", " onclick", " onload"
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
            // Strip the attribute
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
