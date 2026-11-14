use reqwest::cookie::Jar;
use std::sync::Arc;
use url::Url;

/// Builds a `reqwest::cookie::Jar` from either a Netscape/Mozilla `cookies.txt`
/// string or a raw `Cookie:` header string (e.g. `uid=123; pass=abc`).
pub fn build_cookie_jar(raw_cookies: &str, base_url: &str) -> Arc<Jar> {
    let jar = Arc::new(Jar::default());
    let trimmed = raw_cookies.trim();
    if trimmed.is_empty() {
        return jar;
    }

    let Ok(base) = Url::parse(base_url) else {
        return jar;
    };

    let mut added_netscape = 0usize;
    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() || (line.starts_with('#') && !line.starts_with("#HttpOnly_")) {
            continue;
        }

        let clean_line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
        let parts: Vec<&str> = clean_line.split('\t').collect();
        if parts.len() >= 7 {
            let domain = parts[0].trim();
            let path = parts[2].trim();
            let secure = parts[3].trim().eq_ignore_ascii_case("TRUE");
            let name = parts[5].trim();
            let value = parts[6].trim();

            if name.is_empty() {
                continue;
            }

            let cookie_domain = domain.trim_start_matches('.');
            let scheme = if secure || base.scheme() == "https" {
                "https"
            } else {
                "http"
            };
            let target_url = Url::parse(&format!("{scheme}://{cookie_domain}/"))
                .unwrap_or_else(|_| base.clone());

            let mut cookie_str = format!("{name}={value}; Path={path}");
            if domain.starts_with('.') {
                cookie_str.push_str(&format!("; Domain={cookie_domain}"));
            }
            if secure {
                cookie_str.push_str("; Secure");
            }
            jar.add_cookie_str(&cookie_str, &target_url);
            added_netscape += 1;
        }
    }

    // Fallback: raw `Cookie: k1=v1; k2=v2` header format
    if added_netscape == 0 {
        let header_body = trimmed
            .strip_prefix("Cookie:")
            .or_else(|| trimmed.strip_prefix("cookie:"))
            .unwrap_or(trimmed)
            .trim();

        for pair in header_body.split(';') {
            let pair = pair.trim();
            if pair.contains('=') {
                let cookie_str = format!("{pair}; Path=/");
                jar.add_cookie_str(&cookie_str, &base);
            }
        }
    }

    jar
}

/// Domains found in a Netscape `cookies.txt`, normalized (lowercase, no leading `.` or `www.`)
/// and ordered by how many cookies each one holds (most first). Empty for raw `Cookie:` headers,
/// which carry no domain.
pub fn cookie_domains(raw_cookies: &str) -> Vec<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for line in raw_cookies.lines() {
        let line = line.trim();
        if line.is_empty() || (line.starts_with('#') && !line.starts_with("#HttpOnly_")) {
            continue;
        }
        let clean = line.strip_prefix("#HttpOnly_").unwrap_or(line);
        let parts: Vec<&str> = clean.split('\t').collect();
        if parts.len() < 7 || parts[5].trim().is_empty() {
            continue;
        }
        let domain = normalize_host(parts[0]);
        if domain.is_empty() {
            continue;
        }
        match counts.iter_mut().find(|(d, _)| *d == domain) {
            Some((_, n)) => *n += 1,
            None => counts.push((domain, 1)),
        }
    }
    // Stable sort keeps file order among equally common domains.
    counts.sort_by_key(|c| std::cmp::Reverse(c.1));
    counts.into_iter().map(|(d, _)| d).collect()
}

/// Lowercases a host and strips a leading `.` and `www.`.
pub fn normalize_host(host: &str) -> String {
    let h = host.trim().trim_start_matches('.').to_ascii_lowercase();
    h.strip_prefix("www.").map(str::to_string).unwrap_or(h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::cookie::CookieStore;

    #[test]
    fn extracts_cookie_domains_by_frequency() {
        let raw = "# Netscape HTTP Cookie File\n\
.www.Aither.cc\tTRUE\t/\tTRUE\t0\ta\t1\n\
#HttpOnly_aither.cc\tFALSE\t/\tTRUE\t0\tb\t2\n\
cdn.other.net\tFALSE\t/\tTRUE\t0\tc\t3\n";
        assert_eq!(cookie_domains(raw), vec!["aither.cc", "cdn.other.net"]);
        assert!(cookie_domains("Cookie: uid=1; pass=2").is_empty());
    }

    #[test]
    fn parses_netscape_cookies_txt() {
        let raw = r#"
# Netscape HTTP Cookie File
.example.com	TRUE	/	TRUE	1893456000	session_id	abc123
#HttpOnly_example.com	FALSE	/	TRUE	1893456000	uid	42
"#;
        let jar = build_cookie_jar(raw, "https://example.com/");
        let url = Url::parse("https://example.com/inbox.php").unwrap();
        let header = jar.cookies(&url).unwrap();
        let s = header.to_str().unwrap();
        assert!(s.contains("session_id=abc123"));
        assert!(s.contains("uid=42"));
    }

    #[test]
    fn parses_raw_cookie_header_fallback() {
        let raw = "Cookie: uid=99; pass=secret_hash";
        let jar = build_cookie_jar(raw, "https://tracker.test/");
        let url = Url::parse("https://tracker.test/").unwrap();
        let header = jar.cookies(&url).unwrap();
        let s = header.to_str().unwrap();
        assert!(s.contains("uid=99"));
        assert!(s.contains("pass=secret_hash"));
    }
}
