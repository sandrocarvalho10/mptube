/// Verifica se a URL pertence a um domínio permitido.
/// `allowed = None` significa "qualquer domínio é aceito".
pub fn url_allowed(raw_url: &str, allowed: &Option<Vec<String>>) -> bool {
    let Some(allowed) = allowed else {
        return true;
    };

    let Ok(parsed) = url::Url::parse(raw_url) else {
        return false;
    };

    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return false;
    }

    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.to_lowercase();

    allowed
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_listed_domain_and_subdomains() {
        let allowed = Some(vec!["youtube.com".to_string()]);
        assert!(url_allowed("https://youtube.com/watch?v=1", &allowed));
        assert!(url_allowed("https://www.youtube.com/watch?v=1", &allowed));
        assert!(url_allowed("https://m.youtube.com/watch?v=1", &allowed));
    }

    #[test]
    fn rejects_other_domains_and_lookalikes() {
        let allowed = Some(vec!["youtube.com".to_string()]);
        assert!(!url_allowed("https://evil.com/youtube.com", &allowed));
        assert!(!url_allowed("https://notyoutube.com/watch?v=1", &allowed));
        assert!(!url_allowed("not a url", &allowed));
    }

    #[test]
    fn none_allows_everything() {
        assert!(url_allowed("https://anything.example", &None));
    }
}
