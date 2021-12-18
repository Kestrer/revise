use crate::utils::EndpointResult;
use anyhow::Context as _;
use axum::{
    async_trait,
    extract::{FromRequest, RequestParts},
    http::{
        header::{self, HeaderValue},
        StatusCode,
    },
    response::{IntoResponse, Response},
};
use headers::HeaderMapExt as _;
use rand::seq::SliceRandom as _;
use serde::{Deserialize, Serialize};
use sqlx::{
    encode::IsNull,
    postgres::{PgArgumentBuffer, PgTypeInfo},
    Postgres,
};
use std::{str, sync::Arc};

/// A session token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct Session(Arc<str>);

impl Session {
    pub(crate) fn new() -> Self {
        const COOKIE_CHARS: &[u8] =
            b"!#$&'(())+./0123456789:<=?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[]^_`abcdefghijklmnopqrstuvwxyz|~";
        let mut rng = rand::thread_rng();
        let bytes = [(); 32].map(|()| *COOKIE_CHARS.choose(&mut rng).unwrap());
        Self(Arc::from(str::from_utf8(&bytes).unwrap()))
    }

    pub(crate) async fn user_id_http(&self, db: impl sqlx::PgExecutor<'_>) -> EndpointResult<i64> {
        Ok(self
            .user_id(db)
            .await?
            .ok_or_else(|| (StatusCode::UNAUTHORIZED, "session token invalid").into_response())?)
    }

    pub(crate) async fn user_id(
        &self,
        db: impl sqlx::PgExecutor<'_>,
    ) -> anyhow::Result<Option<i64>> {
        Ok(
            sqlx::query_scalar("SELECT for_user FROM session_cookies WHERE cookie_value = $1")
                .bind(self)
                .fetch_optional(db)
                .await
                .context("failed to get user session")?,
        )
    }

    pub(crate) fn set_cookie_on(&self, response: impl IntoResponse) -> Response {
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

    pub(crate) fn clear_cookie_on(response: impl IntoResponse) -> Response {
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
            .and_then(|cookie| cookie.get("session").map(Arc::from))
            .map(Self)
            .ok_or(SessionRejection)
    }
}

impl sqlx::Type<Postgres> for Session {
    fn type_info() -> PgTypeInfo {
        str::type_info()
    }
    fn compatible(ty: &PgTypeInfo) -> bool {
        str::compatible(ty)
    }
}

impl sqlx::Encode<'_, Postgres> for Session {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> IsNull {
        <&str as sqlx::Encode<Postgres>>::encode(&self.0, buf)
    }
}

pub(crate) struct SessionRejection;
impl IntoResponse for SessionRejection {
    fn into_response(self) -> Response {
        (StatusCode::UNAUTHORIZED, "you are not logged in").into_response()
    }
}
