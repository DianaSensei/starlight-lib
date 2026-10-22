use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use starlight_core::pagination::PaginationError;
use validator::{ValidationErrors, ValidationErrorsKind};

use crate::context;

const PROBLEM_CONTENT_TYPE: &str = "application/problem+json";

/// RFC 9457 error body; `code` is the stable, machine-readable key clients branch on.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Problem {
    #[serde(rename = "type")]
    pub problem_type: String,
    pub title: String,
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<FieldError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldError {
    pub field: String,
    pub code: String,
    pub message: String,
}

impl Problem {
    pub fn new(status: StatusCode, code: impl Into<String>, title: impl Into<String>) -> Self {
        let code = code.into();
        Self {
            problem_type: format!("/problems/{code}"),
            title: title.into(),
            status: status.as_u16(),
            detail: None,
            instance: None,
            code,
            request_id: None,
            trace_id: None,
            errors: Vec::new(),
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_errors(mut self, errors: Vec<FieldError>) -> Self {
        self.errors = errors;
        self
    }

    pub fn status_code(&self) -> StatusCode {
        StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
    }

    pub fn malformed_request(detail: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "request.malformed",
            "Malformed request",
        )
        .with_detail(detail)
    }

    pub fn unauthenticated() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "auth.unauthenticated",
            "Authentication required",
        )
    }

    pub fn forbidden() -> Self {
        Self::new(StatusCode::FORBIDDEN, "auth.forbidden", "Permission denied")
    }

    pub fn not_found(code: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, code, "Resource not found")
    }

    pub fn conflict(code: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, "Conflict").with_detail(detail)
    }

    pub fn precondition_required() -> Self {
        Self::new(
            StatusCode::PRECONDITION_REQUIRED,
            "request.precondition_required",
            "Precondition required",
        )
        .with_detail("Send If-Match with the current ETag.")
    }

    pub fn precondition_failed() -> Self {
        Self::new(
            StatusCode::PRECONDITION_FAILED,
            "request.precondition_failed",
            "Precondition failed",
        )
        .with_detail("The resource was modified; fetch it again and retry.")
    }

    pub fn unprocessable(code: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            code,
            "Unprocessable request",
        )
        .with_detail(detail)
    }

    pub fn validation_failed(errors: Vec<FieldError>) -> Self {
        Self::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation.failed",
            "Validation failed",
        )
        .with_detail("One or more fields are invalid.")
        .with_errors(errors)
    }

    pub fn service_unavailable(code: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, code, "Service unavailable")
    }

    /// Logs the cause chain once and returns a body that reveals nothing internal.
    pub fn internal(cause: &(dyn std::error::Error + 'static)) -> Self {
        tracing::error!(error.message = %cause, error.chain = %error_chain(cause), "request failed");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal.error",
            "Internal server error",
        )
    }
}

fn error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    std::iter::successors(Some(error), |current| current.source())
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(": ")
}

impl IntoResponse for Problem {
    fn into_response(mut self) -> Response {
        if let Some(request) = context::current() {
            self.request_id.get_or_insert(request.request_id);
            self.instance.get_or_insert(request.path);
        }
        if self.trace_id.is_none() {
            self.trace_id = starlight_telemetry::current_trace_id();
        }
        let status = self.status_code();
        let mut response = (status, Json(self)).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(PROBLEM_CONTENT_TYPE),
        );
        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
    }
}

impl From<ValidationErrors> for Problem {
    fn from(errors: ValidationErrors) -> Self {
        let mut fields = Vec::new();
        flatten_validation_errors("", &errors, &mut fields);
        fields.sort_by(|left, right| left.field.cmp(&right.field));
        Self::validation_failed(fields)
    }
}

fn flatten_validation_errors(prefix: &str, errors: &ValidationErrors, out: &mut Vec<FieldError>) {
    for (field, kind) in errors.errors() {
        let path = if prefix.is_empty() {
            field.to_string()
        } else {
            format!("{prefix}.{field}")
        };
        match kind {
            ValidationErrorsKind::Field(field_errors) => {
                out.extend(field_errors.iter().map(|error| {
                    FieldError {
                        field: path.clone(),
                        code: error.code.to_string(),
                        message: error
                            .message
                            .as_ref()
                            .map_or_else(|| error.code.to_string(), ToString::to_string),
                    }
                }));
            }
            ValidationErrorsKind::Struct(nested) => flatten_validation_errors(&path, nested, out),
            ValidationErrorsKind::List(items) => {
                for (index, nested) in items {
                    flatten_validation_errors(&format!("{path}[{index}]"), nested, out);
                }
            }
        }
    }
}

impl From<JsonRejection> for Problem {
    fn from(rejection: JsonRejection) -> Self {
        match rejection.status() {
            StatusCode::PAYLOAD_TOO_LARGE => Self::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "request.too_large",
                "Request body too large",
            ),
            StatusCode::UNSUPPORTED_MEDIA_TYPE => Self::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "request.unsupported_media_type",
                "Unsupported media type",
            )
            .with_detail("Use Content-Type: application/json."),
            _ => Self::malformed_request(rejection.body_text()),
        }
    }
}

impl From<QueryRejection> for Problem {
    fn from(rejection: QueryRejection) -> Self {
        Self::malformed_request(rejection.body_text())
    }
}

impl From<PathRejection> for Problem {
    fn from(rejection: PathRejection) -> Self {
        Self::malformed_request(rejection.body_text())
    }
}

impl From<PaginationError> for Problem {
    fn from(error: PaginationError) -> Self {
        Self::malformed_request(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use validator::Validate;

    use super::*;

    #[derive(Validate)]
    struct CreateThing {
        #[validate(length(min = 3, message = "Must be at least 3 characters."))]
        name: String,
        #[validate(range(min = 1))]
        quantity: u32,
    }

    #[test]
    fn validation_errors_become_sorted_field_errors() {
        let errors = CreateThing {
            name: "ab".into(),
            quantity: 0,
        }
        .validate()
        .unwrap_err();

        let problem = Problem::from(errors);

        assert_eq!(problem.status, 422);
        assert_eq!(problem.code, "validation.failed");
        assert_eq!(problem.errors.len(), 2);
        assert_eq!(problem.errors[0].field, "name");
        assert_eq!(problem.errors[0].message, "Must be at least 3 characters.");
        assert_eq!(problem.errors[1].field, "quantity");
        assert_eq!(problem.errors[1].code, "range");
    }

    #[test]
    fn problem_serializes_with_type_and_omits_empty_members() {
        let json = serde_json::to_value(Problem::not_found("store.not_found")).unwrap();

        assert_eq!(json["type"], "/problems/store.not_found");
        assert_eq!(json["status"], 404);
        assert!(json.get("errors").is_none());
        assert!(json.get("detail").is_none());
    }

    #[test]
    fn response_uses_problem_content_type_and_bearer_challenge() {
        let response = Problem::unauthenticated().into_response();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            PROBLEM_CONTENT_TYPE
        );
        assert_eq!(response.headers()[header::WWW_AUTHENTICATE], "Bearer");
    }
}
