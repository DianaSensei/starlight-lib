use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use starlight_core::pagination::Page;

/// `{"data": …}` envelope for a single resource.
#[derive(Debug, Clone, Serialize)]
pub struct DataResponse<T> {
    pub data: T,
}

impl<T: Serialize> DataResponse<T> {
    pub fn new(data: T) -> Self {
        Self { data }
    }

    /// `200 OK` with an `ETag` built from the resource version.
    pub fn with_version(self, version: i32) -> Response {
        let mut response = Json(self).into_response();
        insert_etag(&mut response, version);
        response
    }

    /// `201 Created` with `Location` and `ETag`.
    pub fn created(self, location: &str, version: i32) -> Response {
        let mut response = (StatusCode::CREATED, Json(self)).into_response();
        if let Ok(value) = HeaderValue::from_str(location) {
            response.headers_mut().insert(header::LOCATION, value);
        }
        insert_etag(&mut response, version);
        response
    }
}

impl<T: Serialize> IntoResponse for DataResponse<T> {
    fn into_response(self) -> Response {
        Json(self).into_response()
    }
}

/// `{"data": [...], "page": {...}}` envelope for collections.
#[derive(Debug, Clone, Serialize)]
pub struct PageResponse<T> {
    pub data: Vec<T>,
    pub page: PageInfo,
}

#[derive(Debug, Clone, Serialize)]
pub struct PageInfo {
    pub limit: u32,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

impl<T> From<Page<T>> for PageResponse<T> {
    fn from(page: Page<T>) -> Self {
        let has_more = page.has_more();
        Self {
            data: page.items,
            page: PageInfo {
                limit: page.limit,
                next_cursor: page.next_cursor,
                has_more,
            },
        }
    }
}

impl<T: Serialize> IntoResponse for PageResponse<T> {
    fn into_response(self) -> Response {
        Json(self).into_response()
    }
}

pub fn etag(version: i32) -> String {
    format!("\"{version}\"")
}

fn insert_etag(response: &mut Response, version: i32) {
    if let Ok(value) = HeaderValue::from_str(&etag(version)) {
        response.headers_mut().insert(header::ETAG, value);
    }
}

#[cfg(test)]
mod tests {
    use starlight_core::pagination::{Page, PageRequest};

    use super::*;

    #[test]
    fn page_response_matches_template() {
        let request = PageRequest::new(Some(1), None).unwrap();
        let ids = vec![uuid::Uuid::now_v7(), uuid::Uuid::now_v7()];
        let page = Page::from_overfetched(ids, &request, |id| *id);

        let json = serde_json::to_value(PageResponse::from(page)).unwrap();

        assert_eq!(json["data"].as_array().unwrap().len(), 1);
        assert_eq!(json["page"]["limit"], 1);
        assert_eq!(json["page"]["has_more"], true);
        assert!(json["page"]["next_cursor"].is_string());
    }

    #[test]
    fn created_sets_location_and_etag() {
        let response = DataResponse::new("x").created("/api/v1/things/1", 1);

        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers()[header::LOCATION], "/api/v1/things/1");
        assert_eq!(response.headers()[header::ETAG], "\"1\"");
    }
}
