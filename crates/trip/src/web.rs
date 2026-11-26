//! The AI's window on the internet: `web_search` and `web_read`.
//!
//! Search needs no key by default (DuckDuckGo's HTML page, parsed here); a Brave, SearXNG or
//! Tavily key in `~/.config/tripppin/search` switches to that service. Reading a page fetches
//! it, drops the boilerplate (scripts, menus, footers) and returns plain text, capped in size.

use anyhow::{Result, bail};
use serde_json::Value;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

const MAX_PAGE_BYTES: u64 = 2 << 20;
/// Characters of a page that go back to the model.
pub const MAX_PAGE_CHARS: usize = 6000;
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0 tripppin";

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Which service searches, from `~/.config/tripppin/search` (`provider=`, `key=`, `url=`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchConfig {
    pub provider: String,
    pub key: String,
    pub url: String,
}

pub fn config_file() -> PathBuf {
    crate::paths::config_dir().join("search")
}

pub fn parse_config(text: &str) -> SearchConfig {
    let mut c = SearchConfig::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        match k.trim() {
            "provider" => c.provider = v.trim().to_lowercase(),
            "key" => c.key = v.trim().to_string(),
            "url" => c.url = v.trim().trim_end_matches('/').to_string(),
            _ => {}
        }
    }
    c
}

pub fn load_config() -> SearchConfig {
    std::fs::read_to_string(config_file()).map(|t| parse_config(&t)).unwrap_or_default()
}

pub fn save_config(c: &SearchConfig) -> std::io::Result<()> {
    let file = config_file();
    if let Some(d) = file.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(&file, format!("provider={}\nkey={}\nurl={}\n", c.provider, c.key, c.url))?;
    crate::paths::private(&file);
    Ok(())
}

fn agent() -> ureq::Agent {
    use ureq::tls::{RootCerts, TlsConfig};
    let tls = TlsConfig::builder().root_certs(RootCerts::PlatformVerifier).build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(Duration::from_secs(20)))
        .user_agent(UA)
        .build()
        .into()
}

/// Searches the web with the configured service, or DuckDuckGo without one.
pub fn search(query: &str, limit: usize) -> Result<Vec<Hit>> {
    let c = load_config();
    let agent = agent();
    let hits = match c.provider.as_str() {
        "brave" if !c.key.is_empty() => {
            let mut r = agent
                .get("https://api.search.brave.com/res/v1/web/search")
                .header("X-Subscription-Token", &c.key)
                .header("Accept", "application/json")
                .query("q", query)
                .query("count", limit.to_string())
                .call()?;
            parse_brave(&r.body_mut().read_to_string()?)
        }
        "searxng" if !c.url.is_empty() => {
            let mut r = agent.get(format!("{}/search", c.url)).query("q", query).query("format", "json").call()?;
            parse_searxng(&r.body_mut().read_to_string()?)
        }
        "tavily" if !c.key.is_empty() => {
            let body = serde_json::json!({"api_key": c.key, "query": query, "max_results": limit});
            let mut r = agent
                .post("https://api.tavily.com/search")
                .header("Content-Type", "application/json")
                .send(body.to_string())?;
            parse_tavily(&r.body_mut().read_to_string()?)
        }
        _ => {
            let mut r = agent
                .post("https://html.duckduckgo.com/html/")
                .send_form([("q", query), ("kl", "wt-wt")])
                .map_err(|e| anyhow::anyhow!("the search page didn't load: {e}"))?;
            parse_duckduckgo(&r.body_mut().read_to_string()?)
        }
    };
    Ok(hits.into_iter().take(limit).collect())
}

/// Search results as the model reads them: numbered, one block each.
pub fn format_hits(hits: &[Hit]) -> String {
    hits.iter()
        .enumerate()
        .map(|(i, h)| format!("{}. {}\n   {}\n   {}", i + 1, h.title, h.url, h.snippet))
        .collect::<Vec<_>>()
        .join("\n")
}

// ---- parsers ----------------------------------------------------------------------------

/// DuckDuckGo's HTML results: `<a class="result__a" href="//duckduckgo.com/l/?uddg=URL">title</a>`
/// then `<a class="result__snippet">snippet</a>`.
pub fn parse_duckduckgo(html: &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find("result__a") {
        rest = &rest[i..];
        let Some(tag_end) = rest.find('>') else { break };
        let tag = &rest[..tag_end];
        let href = attr(tag, "href").unwrap_or_default();
        let after = &rest[tag_end + 1..];
        let Some(close) = after.find("</a>") else { break };
        let title = text_of(&after[..close]);
        let next_result = after.find("result__a").unwrap_or(after.len());
        let block = &after[close..next_result];
        let snippet = block
            .find("result__snippet")
            .and_then(|s| {
                let b = &block[s..];
                let st = b.find('>')?;
                let en = b.find("</a>").or_else(|| b.find("</td>")).unwrap_or(b.len());
                Some(text_of(&b[st + 1..en.max(st + 1)]))
            })
            .unwrap_or_default();
        let url = unwrap_ddg(&href);
        if !title.is_empty() && url.starts_with("http") && !url.contains("duckduckgo.com/y.js") {
            hits.push(Hit { title, url, snippet });
        }
        rest = &after[close..];
    }
    hits
}

/// `//duckduckgo.com/l/?uddg=https%3A%2F%2Fa.com&rut=…` → `https://a.com`.
fn unwrap_ddg(href: &str) -> String {
    let href = html_unescape(href);
    if let Some(i) = href.find("uddg=") {
        let v = &href[i + 5..];
        return percent_decode(v.split('&').next().unwrap_or(v));
    }
    if href.starts_with("//") { format!("https:{href}") } else { href }
}

pub fn parse_brave(json: &str) -> Vec<Hit> {
    let v: Value = serde_json::from_str(json).unwrap_or_default();
    v.pointer("/web/results")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|r| Hit { title: s(r, "title"), url: s(r, "url"), snippet: text_of(&s(r, "description")) })
                .collect()
        })
        .unwrap_or_default()
}

pub fn parse_searxng(json: &str) -> Vec<Hit> {
    let v: Value = serde_json::from_str(json).unwrap_or_default();
    v["results"]
        .as_array()
        .map(|a| a.iter().map(|r| Hit { title: s(r, "title"), url: s(r, "url"), snippet: s(r, "content") }).collect())
        .unwrap_or_default()
}

pub fn parse_tavily(json: &str) -> Vec<Hit> {
    let v: Value = serde_json::from_str(json).unwrap_or_default();
    v["results"]
        .as_array()
        .map(|a| a.iter().map(|r| Hit { title: s(r, "title"), url: s(r, "url"), snippet: s(r, "content") }).collect())
        .unwrap_or_default()
}

fn s(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or("").to_string()
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let i = tag.find(&key)? + key.len();
    let j = tag[i..].find('"')?;
    Some(tag[i..i + j].to_string())
}

/// Tags removed, entities decoded, whitespace collapsed.
fn text_of(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    html_unescape(&out).split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn html_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|&e| e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..end];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#x27" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            "hellip" => Some('…'),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            e if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                let h = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                match h {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---- reading a page ---------------------------------------------------------------------

/// Whether `url` is something `web_read` may fetch: http or https only.
pub fn check_url(url: &str) -> Result<(), String> {
    let u = url.trim();
    if !(u.starts_with("http://") || u.starts_with("https://")) || u.contains(char::is_whitespace) {
        return Err("the url must start with http:// or https:// and have no spaces.".into());
    }
    Ok(())
}

/// Fetches a page and returns its readable text, at most `MAX_PAGE_CHARS` long.
pub fn read(url: &str) -> Result<String> {
    check_url(url).map_err(anyhow::Error::msg)?;
    let mut r = agent().get(url.trim()).call().map_err(|e| anyhow::anyhow!("couldn't load {url}: {e}"))?;
    let kind = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_lowercase();
    if !(kind.is_empty()
        || kind.contains("html")
        || kind.contains("text")
        || kind.contains("json")
        || kind.contains("xml"))
    {
        bail!("that's not a web page ({kind}); can't read it as text.");
    }
    let mut buf = Vec::new();
    r.body_mut().as_reader().take(MAX_PAGE_BYTES).read_to_end(&mut buf)?;
    let body = String::from_utf8_lossy(&buf);
    let text =
        if kind.contains("html") || body.trim_start().starts_with('<') { readable(&body) } else { body.to_string() };
    Ok(cap(&text, MAX_PAGE_CHARS))
}

fn cap(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    format!("{cut}\n… (page cut at {max} characters)")
}

/// HTML to readable text: drops scripts, styles, menus, footers and forms; keeps headings,
/// paragraphs and list items on their own lines; prefers `<main>` or `<article>` when present.
pub fn readable(html: &str) -> String {
    let mut h = html.to_string();
    for tag in ["script", "style", "noscript", "svg", "nav", "header", "footer", "aside", "form", "template", "iframe"]
    {
        h = drop_element(&h, tag);
    }
    h = drop_comments(&h);
    for tag in ["main", "article"] {
        if let Some(inner) = element_inner(&h, tag).filter(|i| i.len() > 400) {
            h = inner.to_string();
            break;
        }
    }
    let mut title = String::new();
    if let Some(t) = element_inner(html, "title") {
        title = text_of(t);
    }
    let mut out = String::new();
    let mut in_tag = false;
    let mut tag = String::new();
    for c in h.chars() {
        match c {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let name = tag.trim_start_matches('/').split(|c: char| !c.is_alphanumeric()).next().unwrap_or("");
                match name.to_lowercase().as_str() {
                    "p" | "div" | "br" | "tr" | "section" | "ul" | "ol" | "table" | "h1" | "h2" | "h3" | "h4"
                    | "h5" | "h6" | "pre" | "blockquote" => out.push('\n'),
                    "li" if !tag.starts_with('/') => out.push_str("\n- "),
                    "li" => out.push('\n'),
                    _ => {}
                }
            }
            c if in_tag => tag.push(c),
            c => out.push(c),
        }
    }
    let out = html_unescape(&out);
    let mut lines: Vec<String> = Vec::new();
    for l in out.lines() {
        let l = l.split_whitespace().collect::<Vec<_>>().join(" ");
        if l.is_empty() || l == "-" {
            continue;
        }
        if lines.last() != Some(&l) {
            lines.push(l);
        }
    }
    let body = lines.join("\n");
    if title.is_empty() { body } else { format!("{title}\n\n{body}") }
}

fn drop_element(html: &str, tag: &str) -> String {
    let (open, close) = (format!("<{tag}"), format!("</{tag}>"));
    let lower = html.to_lowercase();
    let mut out = String::with_capacity(html.len());
    let mut pos = 0;
    while let Some(i) = find_open(&lower, &open, pos) {
        out.push_str(&html[pos..i]);
        match lower[i..].find(&close) {
            Some(j) => pos = i + j + close.len(),
            None => {
                pos = html.len();
                break;
            }
        }
    }
    out.push_str(&html[pos.min(html.len())..]);
    out
}

/// `<tag` followed by a space, `>` or `/` (so `<nav` doesn't match `<navigation`).
fn find_open(lower: &str, open: &str, from: usize) -> Option<usize> {
    let mut at = from;
    while let Some(i) = lower[at..].find(open) {
        let p = at + i;
        match lower.as_bytes().get(p + open.len()) {
            Some(b' ' | b'>' | b'/' | b'\n' | b'\t') => return Some(p),
            _ => at = p + open.len(),
        }
    }
    None
}

fn element_inner<'a>(html: &'a str, tag: &str) -> Option<&'a str> {
    let lower = html.to_lowercase();
    let i = find_open(&lower, &format!("<{tag}"), 0)?;
    let start = i + lower[i..].find('>')? + 1;
    let end = start + lower[start..].find(&format!("</{tag}>"))?;
    Some(&html[start..end])
}

fn drop_comments(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(i) = rest.find("<!--") {
        out.push_str(&rest[..i]);
        match rest[i..].find("-->") {
            Some(j) => rest = &rest[i + j + 3..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DDG: &str = r#"
<div class="result results_links results_links_deep web-result">
 <h2 class="result__title"><a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.realpython.com%2Fbeautiful-soup-web-scraper-python%2F&amp;rut=abc">Beautiful Soup: Build a <b>Web Scraper</b> With Python</a></h2>
 <a class="result__snippet" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.realpython.com%2F&amp;rut=abc">In this tutorial, you&#x27;ll learn how to <b>scrape</b> the web.</a>
</div>
<div class="result results_links">
 <h2 class="result__title"><a rel="nofollow" class="result__a" href="https://playwright.dev/python/">Playwright for Python</a></h2>
 <a class="result__snippet" href="https://playwright.dev/python/">Reliable end-to-end testing &amp; automation.</a>
</div>
<div class="result"><a class="result__a" href="//duckduckgo.com/y.js?ad_provider=x">Ad</a></div>
"#;

    #[test]
    fn parses_duckduckgo_results() {
        let hits = parse_duckduckgo(DDG);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].title, "Beautiful Soup: Build a Web Scraper With Python");
        assert_eq!(hits[0].url, "https://www.realpython.com/beautiful-soup-web-scraper-python/");
        assert_eq!(hits[0].snippet, "In this tutorial, you'll learn how to scrape the web.");
        assert_eq!(hits[1].url, "https://playwright.dev/python/");
        assert_eq!(hits[1].snippet, "Reliable end-to-end testing & automation.");
        let text = format_hits(&hits);
        assert!(text.starts_with("1. Beautiful Soup"));
        assert!(text.contains("2. Playwright for Python"));
    }

    #[test]
    fn parses_the_keyed_services() {
        let b = parse_brave(
            r#"{"web":{"results":[{"title":"A","url":"https://a.com","description":"d <strong>x</strong>"}]}}"#,
        );
        assert_eq!(b, vec![Hit { title: "A".into(), url: "https://a.com".into(), snippet: "d x".into() }]);
        let s = parse_searxng(r#"{"results":[{"title":"B","url":"https://b.com","content":"c"}]}"#);
        assert_eq!(s[0].url, "https://b.com");
        let t = parse_tavily(r#"{"results":[{"title":"T","url":"https://t.com","content":"c"}]}"#);
        assert_eq!(t[0].title, "T");
        assert!(parse_brave("not json").is_empty());
    }

    #[test]
    fn parses_config() {
        let c = parse_config("provider = Brave\nkey = k1\nurl=http://x/\n");
        assert_eq!((c.provider.as_str(), c.key.as_str(), c.url.as_str()), ("brave", "k1", "http://x"));
    }

    #[test]
    fn reads_a_page_without_the_boilerplate() {
        let html = "<html><head><title>Hello &amp; welcome</title><style>p{}</style></head><body>\
<nav><a>Home</a><a>About</a></nav><script>var x=1;</script><header>site header</header>\
<main><h1>The Title</h1><p>First paragraph with <a href='#'>a link</a>.</p><ul><li>one</li><li>two</li></ul>\
<p>Second&nbsp;paragraph. Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur.</p></main>\
<footer>copyright</footer></body></html>";
        let t = readable(html);
        assert!(t.starts_with("Hello & welcome"));
        assert!(t.contains("The Title\nFirst paragraph with a link."));
        assert!(t.contains("- one\n- two"));
        assert!(!t.contains("Home") && !t.contains("var x") && !t.contains("copyright") && !t.contains("site header"));
    }

    #[test]
    fn only_web_urls_are_read() {
        assert!(check_url("https://example.com/a").is_ok());
        assert!(check_url("file:///etc/passwd").is_err());
        assert!(check_url("https://a.com/ b").is_err());
        assert_eq!(cap("abcdef", 3), "abc\n… (page cut at 3 characters)");
    }

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(percent_decode("a%20b%2Fc+d%"), "a b/c d%");
        assert_eq!(html_unescape("a &lt; b &#65; &bogus; &"), "a < b A &bogus; &");
    }
}
