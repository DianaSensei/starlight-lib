use axum::http::HeaderMap;

pub const REQUEST_ID_HEADER: &str = "x-request-id";
pub const SUPPORTED_LOCALES: &[&str] = &["en", "vi"];
pub const DEFAULT_LOCALE: &str = "en";
const MAX_REQUEST_ID_LENGTH: usize = 64;

/// Per-request data available anywhere inside the handler future.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestContext {
    pub request_id: String,
    pub path: String,
    pub locale: &'static str,
}

tokio::task_local! {
    static REQUEST_CONTEXT: RequestContext;
}

pub fn current() -> Option<RequestContext> {
    REQUEST_CONTEXT.try_with(Clone::clone).ok()
}

pub(crate) async fn scope<F: Future>(context: RequestContext, future: F) -> F::Output {
    REQUEST_CONTEXT.scope(context, future).await
}

/// Reuses the caller's id only when it is short and log-safe; otherwise mints a UUIDv7.
pub(crate) fn request_id(headers: &HeaderMap) -> String {
    headers
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| is_safe_request_id(value))
        .map_or_else(|| uuid::Uuid::now_v7().to_string(), str::to_owned)
}

fn is_safe_request_id(value: &str) -> bool {
    let has_valid_length = !value.is_empty() && value.len() <= MAX_REQUEST_ID_LENGTH;
    has_valid_length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

pub(crate) fn negotiate_locale(headers: &HeaderMap) -> &'static str {
    let Some(accept_language) = headers
        .get(axum::http::header::ACCEPT_LANGUAGE)
        .and_then(|value| value.to_str().ok())
    else {
        return DEFAULT_LOCALE;
    };
    let preferred = accept_language::parse(accept_language);
    preferred
        .iter()
        .find_map(|tag| {
            let primary = tag.split('-').next().unwrap_or(tag);
            SUPPORTED_LOCALES
                .iter()
                .copied()
                .find(|locale| locale.eq_ignore_ascii_case(primary))
        })
        .unwrap_or(DEFAULT_LOCALE)
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn headers(name: &'static str, value: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(name, HeaderValue::from_static(value));
        headers
    }

    #[test]
    fn safe_request_id_is_reused() {
        assert_eq!(
            request_id(&headers(REQUEST_ID_HEADER, "abc-123")),
            "abc-123"
        );
    }

    #[test]
    fn unsafe_request_id_is_replaced() {
        let generated = request_id(&headers(REQUEST_ID_HEADER, "abc\" injected=1"));

        assert!(uuid::Uuid::parse_str(&generated).is_ok());
    }

    #[test]
    fn locale_matches_primary_subtag_by_preference() {
        assert_eq!(
            negotiate_locale(&headers("accept-language", "vi-VN,vi;q=0.9,en;q=0.8")),
            "vi"
        );
        assert_eq!(
            negotiate_locale(&headers("accept-language", "fr-FR,en;q=0.5")),
            "en"
        );
        assert_eq!(
            negotiate_locale(&headers("accept-language", "ja")),
            DEFAULT_LOCALE
        );
        assert_eq!(negotiate_locale(&HeaderMap::new()), DEFAULT_LOCALE);
    }
}
