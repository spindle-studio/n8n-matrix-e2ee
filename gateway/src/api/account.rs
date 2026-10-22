use axum::{Json, Router, extract::State, http::HeaderMap, routing::post};
use tracing::{error, info};

use crate::gateway::{
    AppState,
    device::{self, DeviceStatusResponse},
};

use super::{
    auth::authorize_request,
    error::{ApiResult, server_error},
};

pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/reset-cross-signing", post(reset_cross_signing))
}

async fn reset_cross_signing(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<DeviceStatusResponse>> {
    let account = authorize_request(&state, &headers)?;

    info!(account = %account.account_id, "resetting Matrix cross-signing identity");
    let status = device::reset_cross_signing(
        &account.account_id,
        &account.matrix_password,
        &account.client,
    )
    .await
    .map_err(|error| {
        error!(account = %account.account_id, %error, "failed to reset Matrix cross-signing identity");
        server_error("failed to reset Matrix cross-signing identity")
    })?;

    Ok(Json(status))
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, StatusCode, header};

    use super::*;

    #[tokio::test]
    async fn reset_cross_signing_rejects_missing_or_invalid_tokens() {
        let state = AppState::from_accounts([]);

        let missing_auth = reset_cross_signing(State(state.clone()), HeaderMap::new()).await;
        assert_eq!(
            missing_auth.expect_err("missing token should fail").0,
            StatusCode::UNAUTHORIZED
        );

        let mut invalid_headers = HeaderMap::new();
        invalid_headers.insert(
            header::AUTHORIZATION,
            "Bearer wrong-token".parse().unwrap(),
        );
        let invalid_auth = reset_cross_signing(State(state), invalid_headers).await;
        assert_eq!(
            invalid_auth.expect_err("invalid token should fail").0,
            StatusCode::UNAUTHORIZED
        );
    }
}
