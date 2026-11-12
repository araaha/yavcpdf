//! Searching the selected text on a website: the sites, their URLs and the query.
use std::collections::BTreeMap;

use serde::Deserialize;

use crate::keymap::parse_sequence;

/// A website to search. `url` holds `{}` where the query goes; empty `keys` disables the site.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebSite {
    pub keys: String,
    #[serde(default)]
    pub url: String,
}

/// `text` with everything but the RFC 3986 unreserved characters written as `%XX`.
pub fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The selection as one line of words, or `None` if it holds none.
pub fn query(selection: &str) -> Option<String> {
    let words: Vec<&str> = selection.split_whitespace().collect();
    (!words.is_empty()).then(|| words.join(" "))
}

/// The URL of a search: `template` with every `{}` replaced by the encoded query.
pub fn search_url(template: &str, query: &str) -> String {
    template.replace("{}", &percent_encode(query))
}

/// Whether the site `name` is usable. A site with no keys is off and not looked at further.
pub fn check(name: &str, site: &WebSite) -> Result<(), String> {
    if site.keys.is_empty() {
        return Ok(());
    }
    parse_sequence(&site.keys).map_err(|e| format!("web_search.{name}: keys: {e}"))?;
    if !(site.url.starts_with("http://") || site.url.starts_with("https://")) {
        return Err(format!("web_search.{name}: url must start with http:// or https://"));
    }
    if !site.url.contains("{}") {
        return Err(format!("web_search.{name}: url must contain {{}}, where the search text goes"));
    }
    Ok(())
}

/// The keys of the sites that are on, by name.
pub fn bindings(sites: &BTreeMap<String, WebSite>) -> BTreeMap<String, String> {
    sites.iter().filter(|(_, s)| !s.keys.is_empty()).map(|(n, s)| (n.clone(), s.keys.clone())).collect()
}

/// A query cut to `max` characters for a message.
pub fn shortened(query: &str, max: usize) -> String {
    if query.chars().count() <= max {
        return query.to_owned();
    }
    let head: String = query.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", head.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encode_keeps_unreserved_and_encodes_the_rest() {
        assert_eq!(percent_encode("Az09-._~"), "Az09-._~");
        assert_eq!(percent_encode("a b"), "a%20b");
        assert_eq!(percent_encode("&?=#+/"), "%26%3F%3D%23%2B%2F");
        assert_eq!(percent_encode("é"), "%C3%A9");
    }

    #[test]
    fn query_collapses_whitespace() {
        assert_eq!(query("  a \n b\t c "), Some("a b c".to_owned()));
        assert_eq!(query(" \n "), None);
        assert_eq!(query(""), None);
    }

    #[test]
    fn search_url_replaces_every_placeholder() {
        assert_eq!(search_url("https://x/?q={}", "a&b"), "https://x/?q=a%26b");
        assert_eq!(search_url("https://x/{}/{}", "a b"), "https://x/a%20b/a%20b");
    }

    #[test]
    fn shortened_cuts_long_text() {
        assert_eq!(shortened("short", 10), "short");
        assert_eq!(shortened("one two three four", 8), "one two…");
    }

    #[test]
    fn check_reports_what_is_wrong() {
        let site = |keys: &str, url: &str| WebSite { keys: keys.to_owned(), url: url.to_owned() };
        assert!(check("a", &site("ss", "https://x/?q={}")).is_ok());
        assert!(check("a", &site("", "nonsense")).is_ok());
        assert!(check("a", &site("ss", "https://x/")).unwrap_err().contains("web_search.a"));
        assert!(check("a", &site("ss", "ftp://x/{}")).is_err());
        assert!(check("a", &site("<nope>", "https://x/{}")).is_err());
    }

    #[test]
    fn percent_encode_handles_empty_percent_and_multibyte_text() {
        assert_eq!(percent_encode(""), "");
        assert_eq!(percent_encode("100%"), "100%25");
        assert_eq!(percent_encode("日本"), "%E6%97%A5%E6%9C%AC");
        assert_eq!(percent_encode("\u{1F600}"), "%F0%9F%98%80");
        assert_eq!(percent_encode("a\nb"), "a%0Ab");
    }

    #[test]
    fn search_url_does_not_substitute_inside_the_query_and_keeps_the_rest() {
        assert_eq!(search_url("https://x/?q={}&y=1", "{}"), "https://x/?q=%7B%7D&y=1");
        assert_eq!(search_url("https://x/", "a"), "https://x/");
    }

    #[test]
    fn shortened_counts_characters_not_bytes() {
        assert_eq!(shortened("ééé", 3), "ééé");
        assert_eq!(shortened("éééé", 3), "éé…");
        assert_eq!(shortened("abc", 0), "abc".chars().take(0).collect::<String>() + "…");
    }

    #[test]
    fn check_rejects_a_site_with_keys_but_no_url() {
        let site = |keys: &str, url: &str| WebSite { keys: keys.to_owned(), url: url.to_owned() };
        let err = check("mine", &site("sm", "")).unwrap_err();
        assert!(err.contains("web_search.mine"), "{err}");
        assert!(check("mine", &site("sm", "https://")).is_err(), "no placeholder");
        assert!(check("mine", &site("", "")).is_ok(), "off");
        assert!(check("mine", &site("", "<nope>")).is_ok(), "off sites are not looked at");
    }

    #[test]
    fn bindings_lists_only_the_sites_that_are_on() {
        let site = |keys: &str| WebSite { keys: keys.to_owned(), url: "https://x/{}".to_owned() };
        let sites = BTreeMap::from([("a".to_owned(), site("sa")), ("b".to_owned(), site(""))]);
        assert_eq!(bindings(&sites), BTreeMap::from([("a".to_owned(), "sa".to_owned())]));
    }
}
