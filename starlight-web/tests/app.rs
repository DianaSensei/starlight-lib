#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::routing::{get, post, put};
use http_body_util::BodyExt;
use serde::Deserialize;
use serde_json::Value;
use starlight_web::{DataResponse, IfMatch, ServerConfig, ValidatedJson, build_app, health_router};
use tower::ServiceExt;
use validator::Validate;

#[derive(Deserialize, Validate)]
#[serde(deny_unknown_fields)]
struct CreateThing {
    #[validate(length(min = 3))]
    name: String,
}

async fn panicking_handler() -> &'static str {
    panic!("boom")
}

fn app() -> Router {
    let api = Router::new()
        .route(
            "/api/v1/things",
            post(
                |ValidatedJson(body): ValidatedJson<CreateThing>| async move {
                    DataResponse::new(serde_json::json!({ "name": body.name }))
                        .created("/api/v1/things/1", 1)
                },
            ),
        )
        .route(
            "/api/v1/things/{id}",
            put(|IfMatch(version): IfMatch| async move { DataResponse::new(version) }),
        )
        .route("/api/v1/panic", get(panicking_handler));
    build_app(api, health_router(Vec::new()), &ServerConfig::default())
}

async fn send(request: Request<Body>) -> (StatusCode, axum::http::HeaderMap, Value) {
    let response = app().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, headers, body)
}

fn json_post(uri: &str, body: &'static str) -> Request<Body> {
    Request::post(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap()
}

#[tokio::test]
async fn unknown_route_returns_problem_with_request_context() {
    let request = Request::get("/api/v1/missing")
        .header("x-request-id", "req-1")
        .body(Body::empty())
        .unwrap();

    let (status, headers, body) = send(request).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(headers[header::CONTENT_TYPE], "application/problem+json");
    assert_eq!(headers["x-request-id"], "req-1");
    assert_eq!(body["code"], "route.not_found");
    assert_eq!(body["request_id"], "req-1");
    assert_eq!(body["instance"], "/api/v1/missing");
}

#[tokio::test]
async fn valid_body_is_created_with_location() {
    let (status, headers, body) = send(json_post("/api/v1/things", r#"{"name":"lamp"}"#)).await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(headers[header::LOCATION], "/api/v1/things/1");
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(body["data"]["name"], "lamp");
}

#[tokio::test]
async fn invalid_body_returns_validation_problem() {
    let (status, _, body) = send(json_post("/api/v1/things", r#"{"name":"x"}"#)).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "validation.failed");
    assert_eq!(body["errors"][0]["field"], "name");
}

#[tokio::test]
async fn malformed_or_unknown_fields_return_bad_request() {
    let (syntax_status, _, syntax_body) = send(json_post("/api/v1/things", "{")).await;
    let (unknown_status, _, _) =
        send(json_post("/api/v1/things", r#"{"name":"lamp","extra":1}"#)).await;

    assert_eq!(syntax_status, StatusCode::BAD_REQUEST);
    assert_eq!(syntax_body["code"], "request.malformed");
    assert_eq!(unknown_status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn update_requires_if_match() {
    let missing = Request::put("/api/v1/things/1")
        .body(Body::empty())
        .unwrap();
    let present = Request::put("/api/v1/things/1")
        .header(header::IF_MATCH, "\"7\"")
        .body(Body::empty())
        .unwrap();

    let (missing_status, _, missing_body) = send(missing).await;
    let (present_status, _, present_body) = send(present).await;

    assert_eq!(missing_status, StatusCode::PRECONDITION_REQUIRED);
    assert_eq!(missing_body["code"], "request.precondition_required");
    assert_eq!(present_status, StatusCode::OK);
    assert_eq!(present_body["data"], 7);
}

#[tokio::test]
async fn panic_becomes_internal_problem() {
    let (status, _, body) = send(Request::get("/api/v1/panic").body(Body::empty()).unwrap()).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["code"], "internal.error");
}

#[tokio::test]
async fn health_endpoints_respond() {
    let (live_status, _, live) =
        send(Request::get("/health/live").body(Body::empty()).unwrap()).await;
    let (ready_status, _, ready) =
        send(Request::get("/health/ready").body(Body::empty()).unwrap()).await;

    assert_eq!(live_status, StatusCode::OK);
    assert_eq!(live["status"], "ok");
    assert_eq!(ready_status, StatusCode::OK);
    assert_eq!(ready["status"], "ok");
}
