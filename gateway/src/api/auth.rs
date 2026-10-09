use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};

use crate::gateway::AppState;

use super::{
    error::ErrorResponse,
    error::{ApiResult, unauthorized_error},
};

pub(crate) async fn auth_check(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    authorize_request(&state.api_secret_token, &headers)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) fn authorize_request(expected_token: &str, headers: &HeaderMap) -> ApiResult<()> {
    if authorization_header_token(headers).is_some_and(|token| token == expected_token) {
        return Ok(());
    }

    Err(unauthorized_error("missing or invalid gateway token"))
}

fn authorization_header_token(headers: &HeaderMap) -> Option<&str> {
    let header_value = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let (scheme, token) = header_value.split_once(' ')?;

    if !scheme.eq_ignore_ascii_case("Bearer") {
        return None;
    }

    let token = token.trim();
    (!token.is_empty()).then_some(token)
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, header};

    use super::*;

    #[test]
    fn bearer_authorization_header_extracts_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            "Bearer secret-token".parse().unwrap(),
        );

        assert_eq!(authorization_header_token(&headers), Some("secret-token"));
    }

    #[test]
    fn bearer_authorization_header_rejects_invalid_values() {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "Basic abc123".parse().unwrap());

        assert_eq!(authorization_header_token(&headers), None);
    }

    #[test]
    fn request_authorization_accepts_bearer_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            "Bearer secret-token".parse().unwrap(),
        );

        assert!(authorize_request("secret-token", &headers).is_ok());
    }

    #[test]
    fn request_authorization_rejects_missing_credentials() {
        assert_eq!(
            authorize_request("secret-token", &HeaderMap::new())
                .expect_err("missing tokens should be rejected")
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
}
