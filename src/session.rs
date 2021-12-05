use anyhow::Context as _;
use axum::{
    async_trait,
    body::BoxBody,
    extract::{FromRequest, RequestParts},
    http::{
        header::{self, HeaderValue},
        Response, StatusCode,
    },
    response::IntoResponse,
};
use headers::HeaderMapExt as _;
use rand::seq::SliceRandom as _;
use sqlx::Postgres;

use crate::utils::EndpointResult;

/// A session token.
#[derive(sqlx::Type)]
#[sqlx(transparent)]
pub(crate) struct Session(String);

impl Session {
    pub(crate) fn new() -> Self {
        const COOKIE_CHARS: &[u8] =
            b"!#$&'(())+./0123456789:<=?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[]^_`abcdefghijklmnopqrstuvwxyz|~";

        let mut rng = rand::thread_rng();
        Self(
            String::from_utf8(
                (0..32)
                    .map(|_| *COOKIE_CHARS.choose(&mut rng).unwrap())
                    .collect(),
            )
            .unwrap(),
        )
    }

    pub(crate) async fn user_id(
        &self,
        db: impl sqlx::Executor<'_, Database = Postgres>,
    ) -> EndpointResult<i64> {
        Ok(
            sqlx::query_scalar("SELECT for_user FROM session_cookies WHERE cookie_value = $1")
                .bind(self)
                .fetch_optional(db)
                .await
                .context("failed to get user session")?
                .ok_or_else(|| {
                    (StatusCode::UNAUTHORIZED, "session token invalid").into_response()
                })?,
        )
    }

    pub(crate) fn set_cookie_on(&self, response: impl IntoResponse) -> Response<BoxBody> {
        let mut response = response.into_response();
        response.headers_mut().insert(
            header::SET_COOKIE,
            HeaderValue::from_str(&format!(
                "session={};Max-Age={};Secure;HttpOnly;Path=/",
                &self.0,
                60 * 60 * 24 * 365
            ))
            .unwrap(),
        );
        response
    }

    pub(crate) fn clear_cookie_on(&self, response: impl IntoResponse) -> Response<BoxBody> {
        let mut response = response.into_response();
        response.headers_mut().insert(
            header::SET_COOKIE,
            HeaderValue::from_static("session=;Max-Age=0;Path=/"),
        );
        response
    }
}

#[async_trait]
impl<B: Send> FromRequest<B> for Session {
    type Rejection = SessionRejection;
    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        req.headers()
            .and_then(|headers| headers.typed_get::<headers::Cookie>())
            .and_then(|cookie| cookie.get("session").map(str::to_owned))
            .map(Self)
            .ok_or(SessionRejection)
    }
}

pub(crate) struct SessionRejection;
impl IntoResponse for SessionRejection {
    fn into_response(self) -> Response<BoxBody> {
        (StatusCode::UNAUTHORIZED, "you are not logged in").into_response()
    }
}
