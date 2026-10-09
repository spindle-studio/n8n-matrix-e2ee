use axum::{Json, http::StatusCode};
use serde::Serialize;

pub(crate) type ApiError = (StatusCode, Json<ErrorResponse>);
pub(crate) type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug, Serialize)]
pub(crate) struct ErrorResponse {
    error: String,
}

pub(crate) fn client_error(message: &str) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: message.to_owned(),
        }),
    )
}

pub(crate) fn server_error(message: &str) -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: message.to_owned(),
        }),
    )
}

pub(crate) fn unauthorized_error(message: &str) -> ApiError {
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorResponse {
            error: message.to_owned(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_responses_have_expected_statuses() {
        assert_eq!(client_error("bad").0, StatusCode::BAD_REQUEST);
        assert_eq!(server_error("bad").0, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(unauthorized_error("bad").0, StatusCode::UNAUTHORIZED);
    }
}
