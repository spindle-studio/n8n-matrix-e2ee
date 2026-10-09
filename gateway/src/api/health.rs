use axum::http::StatusCode;

pub(crate) async fn healthz() -> StatusCode {
    StatusCode::OK
}
