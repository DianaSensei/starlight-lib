use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::http::header;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use starlight_core::pagination::PageRequest;
use validator::Validate;

use crate::problem::Problem;

/// JSON body whose rejections are Problem Details.
pub struct Json<T>(pub T);

impl<S, T> FromRequest<S> for Json<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = Problem;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let axum::Json(value) = axum::Json::<T>::from_request(request, state).await?;
        Ok(Self(value))
    }
}

impl<T: serde::Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        axum::Json(self.0).into_response()
    }
}

/// JSON body that must also pass `validator` rules; failures become `422 validation.failed`.
pub struct ValidatedJson<T>(pub T);

impl<S, T> FromRequest<S> for ValidatedJson<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = Problem;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(request, state).await?;
        value.validate()?;
        Ok(Self(value))
    }
}

/// Query string whose rejections are Problem Details.
pub struct Query<T>(pub T);

impl<S, T> FromRequestParts<S> for Query<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = Problem;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let axum::extract::Query(value) =
            axum::extract::Query::<T>::from_request_parts(parts, state).await?;
        Ok(Self(value))
    }
}

/// Path parameters whose rejections are Problem Details.
pub struct Path<T>(pub T);

impl<S, T> FromRequestParts<S> for Path<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = Problem;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let axum::extract::Path(value) =
            axum::extract::Path::<T>::from_request_parts(parts, state).await?;
        Ok(Self(value))
    }
}

#[derive(Debug, Deserialize)]
struct PageParams {
    limit: Option<u32>,
    cursor: Option<String>,
}

/// `?limit=&cursor=` validated against the shared pagination bounds.
pub struct Pagination(pub PageRequest);

impl<S: Send + Sync> FromRequestParts<S> for Pagination {
    type Rejection = Problem;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Query(params) = Query::<PageParams>::from_request_parts(parts, state).await?;
        Ok(Self(PageRequest::new(
            params.limit,
            params.cursor.as_deref(),
        )?))
    }
}

/// Version from `If-Match: "<version>"`, required for optimistic concurrency on updates.
pub struct IfMatch(pub i32);

impl<S: Send + Sync> FromRequestParts<S> for IfMatch {
    type Rejection = Problem;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let value = parts
            .headers
            .get(header::IF_MATCH)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(Problem::precondition_required)?;
        let version = value
            .trim()
            .trim_start_matches("W/")
            .trim_matches('"')
            .parse()
            .map_err(|_| {
                Problem::malformed_request("If-Match must be an ETag returned by this API.")
            })?;
        Ok(Self(version))
    }
}
