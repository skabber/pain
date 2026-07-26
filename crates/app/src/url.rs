//! Finding clickable URLs in terminal output.
//!
//! Detection is deliberately conservative. A terminal line is arbitrary
//! program output, not markup, so the cost of a false positive (opening a
//! browser at something that only looked like a link) is higher than the
//! cost of a miss. Only explicit, well-known schemes count — a bare
//! `example.com` or `www.example.com` is not treated as a link.

/// Schemes worth making clickable. `file:` is deliberately absent: a
/// terminal prints paths constantly, and one-click-opening a local file
/// or directory from arbitrary output is a much easier thing to trigger
/// by accident than opening a web page.
const SCHEMES: &[&str] = &["https://", "http://", "ftp://", "ssh://", "mailto:"];

/// Characters that commonly sit next to a URL in prose or program output
/// but are almost never meant as part of it — a URL at the end of a
/// sentence, or wrapped in brackets/quotes by a log formatter.
const TRAILING_NOISE: &[char] = &['.', ',', ';', ':', '!', '?', ')', ']', '}', '>', '"', '\'', '`'];

/// A URL found in a line, as a half-open range of character indices plus
/// the URL text itself. Indices are into the line's `char`s (which is
/// what a terminal grid column maps to), not bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub start: usize,
    pub end: usize,
    pub url: String,
}

/// Finds every URL in `line`.
pub fn find(line: &str) -> Vec<Match> {
    let chars: Vec<char> = line.chars().collect();
    let lower: String = line.to_lowercase();
    let lower_chars: Vec<char> = lower.chars().collect();

    let mut matches: Vec<Match> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let Some(scheme) = SCHEMES.iter().find(|s| starts_with_at(&lower_chars, i, s)) else {
            i += 1;
            continue;
        };
        // Run to the first character that can't be in a URL. Whitespace
        // ends it; so does a control character, which in terminal output
        // usually means a rendering artifact rather than real content.
        let mut end = i + scheme.chars().count();
        while end < chars.len() && !chars[end].is_whitespace() && !chars[end].is_control() {
            end += 1;
        }
        // Nothing after the scheme is not a link — `https://` alone is
        // just text.
        if end == i + scheme.chars().count() {
            i = end;
            continue;
        }
        while end > i && TRAILING_NOISE.contains(&chars[end - 1]) {
            end -= 1;
        }
        // A closing paren is only noise if it isn't balancing one inside
        // the URL — Wikipedia-style links genuinely end in `)`.
        let url: String = chars[i..end].iter().collect();
        matches.push(Match { start: i, end, url });
        i = end;
    }
    matches
}

fn starts_with_at(haystack: &[char], at: usize, needle: &str) -> bool {
    let needle: Vec<char> = needle.chars().collect();
    if at + needle.len() > haystack.len() {
        return false;
    }
    haystack[at..at + needle.len()] == needle[..]
}

/// The URL at character index `col` in `line`, if any — what a click on a
/// given grid column resolves to.
pub fn at_column(line: &str, col: usize) -> Option<String> {
    find(line).into_iter().find(|m| col >= m.start && col < m.end).map(|m| m.url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_plain_https_url() {
        let m = find("see https://example.com/path for details");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].url, "https://example.com/path");
    }

    #[test]
    fn finds_several_urls_on_one_line() {
        let m = find("https://a.test and http://b.test");
        assert_eq!(m.iter().map(|m| m.url.as_str()).collect::<Vec<_>>(), vec!["https://a.test", "http://b.test"]);
    }

    #[test]
    fn strips_sentence_punctuation_but_keeps_path_characters() {
        assert_eq!(find("go to https://example.com/a_b-c?q=1.")[0].url, "https://example.com/a_b-c?q=1");
        assert_eq!(find("(https://example.com/x)")[0].url, "https://example.com/x");
    }

    #[test]
    fn ignores_bare_domains_without_a_scheme() {
        // Conservative on purpose: output is full of things that look
        // domain-ish, and a wrong browser launch is worse than a miss.
        assert!(find("visit example.com or www.example.com").is_empty());
    }

    #[test]
    fn ignores_a_scheme_with_nothing_after_it() {
        assert!(find("the https:// prefix").is_empty());
    }

    #[test]
    fn does_not_treat_local_paths_as_links() {
        assert!(find("file:///etc/passwd").is_empty());
        assert!(find("/usr/bin/pain and ./relative/path").is_empty());
    }

    #[test]
    fn scheme_matching_is_case_insensitive_but_the_url_keeps_its_case() {
        let m = find("HTTPS://Example.COM/Path");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].url, "HTTPS://Example.COM/Path");
    }

    #[test]
    fn at_column_resolves_only_inside_the_url() {
        // "x " occupies columns 0-1, so the 19-character URL spans 2..21.
        let line = "x https://example.com y";
        assert_eq!(at_column(line, 0), None, "before the url");
        assert_eq!(at_column(line, 2).as_deref(), Some("https://example.com"), "first url char");
        assert_eq!(at_column(line, 20).as_deref(), Some("https://example.com"), "last url char");
        assert_eq!(at_column(line, 21), None, "the space after the url");
        assert_eq!(at_column(line, 22), None, "past the url");
    }

    #[test]
    fn column_indices_are_characters_not_bytes() {
        // A multi-byte prefix must not shift the reported columns — grid
        // columns are characters, and getting this wrong would make links
        // unclickable on any line containing non-ASCII output.
        let line = "→→ https://example.com";
        let m = &find(line)[0];
        assert_eq!(m.start, 3);
        assert_eq!(at_column(line, 3).as_deref(), Some("https://example.com"));
    }
}
