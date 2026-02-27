use std::time::Duration;
use tracing::debug;

const FETCH_TIMEOUT_SECONDS: u64 = 20;
const MAX_FETCH_BYTES: usize = 1_000_000; // 1 MB
const WEB_USER_AGENT: &str = "crabclaw/0.1";

/// Normalize a raw URL string, prepending `https://` if no scheme is present.
fn normalize_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        Some(trimmed.to_string())
    } else {
        Some(format!("https://{trimmed}"))
    }
}

/// Fetch a URL and return the content as Markdown-like text.
///
/// - HTML responses are converted to a simplified Markdown format.
/// - Plain text is returned as-is.
/// - Responses larger than 1 MB are truncated.
///
/// Uses `reqwest::blocking` wrapped in a dedicated OS thread so it works
/// safely from within a tokio async runtime (avoids both deadlocks and
/// "Cannot start a runtime from within a runtime" panics).
pub fn fetch_url(raw_url: &str) -> String {
    let url = match normalize_url(raw_url) {
        Some(u) => u,
        None => return "Error: empty URL".to_string(),
    };

    debug!("web.fetch: {url}");

    // Spawn a dedicated OS thread for the blocking HTTP request.
    // reqwest::blocking internally creates its own tokio runtime, which
    // panics if called from within an existing tokio runtime. By moving
    // the call to a separate thread, we avoid this issue entirely.
    let result = std::thread::spawn(move || -> Result<String, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(FETCH_TIMEOUT_SECONDS))
            .redirect(reqwest::redirect::Policy::limited(5))
            .user_agent(WEB_USER_AGENT)
            .build()
            .map_err(|e| format!("Error: failed to create HTTP client: {e}"))?;

        let response = client
            .get(&url)
            .send()
            .map_err(|e| format!("Error: HTTP request failed: {e}"))?;

        let status = response.status();
        if !status.is_success() {
            return Err(format!("Error: HTTP {status}"));
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();

        let bytes = response
            .bytes()
            .map_err(|e| format!("Error: failed to read response: {e}"))?;

        let truncated = bytes.len() > MAX_FETCH_BYTES;
        let text = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_FETCH_BYTES)]);

        let rendered = if content_type.contains("text/html") {
            strip_html_to_markdown(&text)
        } else {
            text.to_string()
        };

        if rendered.trim().is_empty() {
            return Err("Error: empty response body".to_string());
        }

        if truncated {
            Ok(format!(
                "{rendered}\n\n[truncated: response exceeded {MAX_FETCH_BYTES} bytes]"
            ))
        } else {
            Ok(rendered)
        }
    })
    .join()
    .unwrap_or_else(|_| Err("Error: fetch thread panicked".to_string()));

    match result {
        Ok(content) => content,
        Err(err) => err,
    }
}

/// Generate a DuckDuckGo search URL for the given query.
pub fn web_search(query: &str) -> String {
    let encoded = urlencoding::encode(query);
    format!(
        "Search URL: https://duckduckgo.com/?q={encoded}\n\nTip: Use web.fetch to retrieve the content of specific result pages."
    )
}

/// Convert HTML to a simplified Markdown-like text.
///
/// This is a lightweight converter that handles common HTML elements
/// without pulling in a full HTML parser dependency.
pub fn strip_html_to_markdown(html: &str) -> String {
    let mut result = String::with_capacity(html.len() / 2);
    let mut chars = html.chars().peekable();

    // Track state
    let mut in_tag = false;
    let mut tag_buf = String::new();
    let mut skip_content = false; // for <script>, <style>, etc.
    let mut skip_tag_name = String::new();
    let mut last_was_newline = false;

    while let Some(ch) = chars.next() {
        if ch == '<' {
            in_tag = true;
            tag_buf.clear();
            continue;
        }

        if in_tag {
            if ch == '>' {
                in_tag = false;
                let tag = tag_buf.trim().to_lowercase();
                let tag_name = tag
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_start_matches('/');

                // Handle skip regions
                if tag.starts_with("script")
                    || tag.starts_with("style")
                    || tag.starts_with("nav")
                    || tag.starts_with("footer")
                    || tag.starts_with("header")
                    || tag.starts_with("noscript")
                {
                    skip_content = true;
                    skip_tag_name = tag_name.to_string();
                    continue;
                }
                if tag.starts_with('/') && skip_content {
                    let closing = tag.trim_start_matches('/');
                    let closing_name = closing.split_whitespace().next().unwrap_or("");
                    if closing_name == skip_tag_name {
                        skip_content = false;
                        skip_tag_name.clear();
                    }
                    continue;
                }
                if skip_content {
                    continue;
                }

                // Convert tags to markdown
                match tag_name {
                    "h1" => result.push_str("\n# "),
                    "h2" => result.push_str("\n## "),
                    "h3" => result.push_str("\n### "),
                    "h4" => result.push_str("\n#### "),
                    "h5" => result.push_str("\n##### "),
                    "h6" => result.push_str("\n###### "),
                    "p" | "div" | "section" | "article" => {
                        if !result.ends_with('\n') {
                            result.push('\n');
                        }
                        if !last_was_newline {
                            result.push('\n');
                            last_was_newline = true;
                        }
                    }
                    "br" => {
                        result.push('\n');
                        last_was_newline = true;
                    }
                    "li" => result.push_str("\n- "),
                    "strong" | "b" => result.push_str("**"),
                    "em" | "i" => result.push('*'),
                    "code" => result.push('`'),
                    "pre" => result.push_str("\n```\n"),
                    "hr" => result.push_str("\n---\n"),
                    _ if tag.starts_with("a ") => {
                        // Extract href from <a href="...">
                        if let Some(href) = extract_attr(&tag_buf, "href") {
                            result.push('[');
                            // We'll close this when we hit </a>
                            // For now, just mark it — the text content follows
                            // Store href to use at closing tag
                            // Simple approach: just emit [text](url) inline
                            // We'll handle this by not emitting href here
                            // and just wrapping text
                            let _ = href; // href handled at closing
                        }
                    }
                    _ if tag.starts_with("/a") => {
                        result.push(']');
                        // We can't easily retrieve the href from earlier,
                        // so just close the bracket
                    }
                    _ if tag.starts_with("/strong") || tag.starts_with("/b") => {
                        result.push_str("**");
                    }
                    _ if tag.starts_with("/em") || tag.starts_with("/i") => {
                        result.push('*');
                    }
                    _ if tag.starts_with("/code") => {
                        result.push('`');
                    }
                    _ if tag.starts_with("/pre") => {
                        result.push_str("\n```\n");
                    }
                    _ if tag.starts_with("/h") => {
                        result.push('\n');
                        last_was_newline = true;
                    }
                    _ if tag.starts_with("/p")
                        || tag.starts_with("/div")
                        || tag.starts_with("/li") =>
                    {
                        if !result.ends_with('\n') {
                            result.push('\n');
                        }
                    }
                    _ => {} // strip unknown tags
                }
            } else {
                tag_buf.push(ch);
            }
            continue;
        }

        if skip_content {
            continue;
        }

        // Decode common HTML entities
        if ch == '&' {
            let mut entity = String::new();
            for ech in chars.by_ref() {
                if ech == ';' {
                    break;
                }
                entity.push(ech);
                if entity.len() > 10 {
                    // Not a real entity, just emit as-is
                    result.push('&');
                    result.push_str(&entity);
                    entity.clear();
                    break;
                }
            }
            if !entity.is_empty() {
                match entity.as_str() {
                    "amp" => result.push('&'),
                    "lt" => result.push('<'),
                    "gt" => result.push('>'),
                    "quot" => result.push('"'),
                    "apos" | "#39" => result.push('\''),
                    "nbsp" => result.push(' '),
                    "#160" => result.push(' '),
                    _ => {
                        result.push('&');
                        result.push_str(&entity);
                        result.push(';');
                    }
                }
            }
            last_was_newline = false;
            continue;
        }

        // Collapse multiple whitespace
        if ch == '\n' || ch == '\r' {
            if !last_was_newline && !result.is_empty() {
                result.push('\n');
                last_was_newline = true;
            }
        } else if ch == ' ' || ch == '\t' {
            if !result.ends_with(' ') && !result.ends_with('\n') {
                result.push(' ');
            }
        } else {
            result.push(ch);
            last_was_newline = false;
        }
    }

    // Clean up excessive newlines
    let mut cleaned = String::with_capacity(result.len());
    let mut newline_count = 0;
    for ch in result.chars() {
        if ch == '\n' {
            newline_count += 1;
            if newline_count <= 2 {
                cleaned.push(ch);
            }
        } else {
            newline_count = 0;
            cleaned.push(ch);
        }
    }

    cleaned.trim().to_string()
}

/// Extract an attribute value from an HTML tag string.
fn extract_attr(tag: &str, attr_name: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    let pattern = format!("{attr_name}=\"");
    if let Some(start) = lower.find(&pattern) {
        let value_start = start + pattern.len();
        if let Some(end) = lower[value_start..].find('"') {
            return Some(tag[value_start..value_start + end].to_string());
        }
    }
    // Also handle single quotes
    let pattern_sq = format!("{attr_name}='");
    if let Some(start) = lower.find(&pattern_sq) {
        let value_start = start + pattern_sq.len();
        if let Some(end) = lower[value_start..].find('\'') {
            return Some(tag[value_start..value_start + end].to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_url_adds_https() {
        assert_eq!(
            normalize_url("example.com"),
            Some("https://example.com".to_string())
        );
    }

    #[test]
    fn normalize_url_preserves_http() {
        assert_eq!(
            normalize_url("http://example.com"),
            Some("http://example.com".to_string())
        );
    }

    #[test]
    fn normalize_url_preserves_https() {
        assert_eq!(
            normalize_url("https://example.com"),
            Some("https://example.com".to_string())
        );
    }

    #[test]
    fn normalize_url_empty_returns_none() {
        assert_eq!(normalize_url(""), None);
        assert_eq!(normalize_url("   "), None);
    }

    #[test]
    fn strip_html_headers() {
        let html = "<h1>Title</h1><h2>Subtitle</h2>";
        let md = strip_html_to_markdown(html);
        assert!(md.contains("# Title"), "h1: {md}");
        assert!(md.contains("## Subtitle"), "h2: {md}");
    }

    #[test]
    fn strip_html_paragraphs() {
        let html = "<p>First paragraph.</p><p>Second paragraph.</p>";
        let md = strip_html_to_markdown(html);
        assert!(md.contains("First paragraph."), "p1: {md}");
        assert!(md.contains("Second paragraph."), "p2: {md}");
    }

    #[test]
    fn strip_html_bold_italic() {
        let html = "<b>bold</b> and <i>italic</i>";
        let md = strip_html_to_markdown(html);
        assert!(md.contains("**bold**"), "bold: {md}");
        assert!(md.contains("*italic*"), "italic: {md}");
    }

    #[test]
    fn strip_html_code() {
        let html = "Use <code>cargo test</code> to run";
        let md = strip_html_to_markdown(html);
        assert!(md.contains("`cargo test`"), "code: {md}");
    }

    #[test]
    fn strip_html_list() {
        let html = "<ul><li>Item 1</li><li>Item 2</li></ul>";
        let md = strip_html_to_markdown(html);
        assert!(md.contains("- Item 1"), "li1: {md}");
        assert!(md.contains("- Item 2"), "li2: {md}");
    }

    #[test]
    fn strip_html_removes_script_style() {
        let html = "<p>Keep</p><script>var x = 1;</script><style>.foo{}</style><p>Also keep</p>";
        let md = strip_html_to_markdown(html);
        assert!(md.contains("Keep"), "keep: {md}");
        assert!(md.contains("Also keep"), "also keep: {md}");
        assert!(!md.contains("var x"), "script removed: {md}");
        assert!(!md.contains(".foo"), "style removed: {md}");
    }

    #[test]
    fn strip_html_entities() {
        let html = "&amp; &lt; &gt; &quot; &nbsp;";
        let md = strip_html_to_markdown(html);
        assert!(md.contains('&'), "amp: {md}");
        assert!(md.contains('<'), "lt: {md}");
        assert!(md.contains('>'), "gt: {md}");
    }

    #[test]
    fn web_search_returns_duckduckgo_url() {
        let result = web_search("rust programming");
        assert!(result.contains("duckduckgo.com"), "has DDG URL: {result}");
        assert!(
            result.contains("rust+programming") || result.contains("rust%20programming"),
            "query encoded: {result}"
        );
    }

    #[test]
    fn extract_attr_href() {
        let tag = r#"a href="https://example.com" class="link""#;
        assert_eq!(
            extract_attr(tag, "href"),
            Some("https://example.com".to_string())
        );
    }

    #[test]
    fn extract_attr_missing() {
        let tag = "a class=\"link\"";
        assert_eq!(extract_attr(tag, "href"), None);
    }
}
