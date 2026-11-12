use super::*;

pub struct AppErr(pub String);
impl<E: std::fmt::Display> From<E> for AppErr {
    fn from(e: E) -> Self {
        AppErr(e.to_string())
    }
}
impl IntoResponse for AppErr {
    fn into_response(self) -> Response {
        eprintln!("error: {}", self.0);
        (StatusCode::INTERNAL_SERVER_ERROR, "Error interno").into_response()
    }
}
pub type R = Result<Response, AppErr>;

pub fn bad(msg: &'static str) -> R {
    Ok((StatusCode::BAD_REQUEST, msg).into_response())
}
