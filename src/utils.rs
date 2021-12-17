use std::ops::{Deref, DerefMut};

use anyhow::Context as _;
use axum::{
    async_trait,
    body::{self, Bytes, HttpBody},
    extract::{FromRequest, RequestParts},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{
    de::{self, Error as _},
    Deserialize, Deserializer,
};
use sqlx::{PgPool, Postgres, Transaction};

pub(crate) struct EndpointError(Response);

impl EndpointError {
    pub(crate) fn new<R: IntoResponse>(response: R) -> Self {
        Self(response.into_response())
    }
}

impl<B> From<Response<B>> for EndpointError
where
    B: HttpBody<Data = Bytes> + Send + 'static,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    fn from(response: Response<B>) -> Self {
        Self(response.map(body::boxed))
    }
}

impl From<anyhow::Error> for EndpointError {
    fn from(error: anyhow::Error) -> Self {
        log::error!("internal server error: {:?}", error);
        Self::from(
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal server error. Try reloading the page.",
            )
                .into_response(),
        )
    }
}

impl IntoResponse for EndpointError {
    fn into_response(self) -> Response {
        self.0
    }
}

pub(crate) type EndpointResult<T = Response> = Result<T, EndpointError>;

#[derive(sqlx::Type)]
#[sqlx(transparent)]
pub(crate) struct NonEmptyString(String);
impl<'de> Deserialize<'de> for NonEmptyString {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        if s.is_empty() {
            return Err(D::Error::invalid_value(
                de::Unexpected::Str(&s),
                &"a non-empty string",
            ));
        }
        Ok(Self(s))
    }
}
impl Deref for NonEmptyString {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(sqlx::Type)]
#[sqlx(transparent)]
pub(crate) struct NonBlankString(String);
impl<'de> Deserialize<'de> for NonBlankString {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        if s.chars().all(|c| c.is_whitespace()) {
            return Err(D::Error::custom("string is blank"));
        }
        Ok(Self(s))
    }
}

pub(crate) struct ReqTransaction(Transaction<'static, Postgres>);

#[async_trait]
impl<B: Send + Sync> FromRequest<B> for ReqTransaction {
    type Rejection = EndpointError;
    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        let transaction = req
            .extensions()
            .context("extensions taken")?
            .get::<PgPool>()
            .context("no PgPool extension")?
            .begin()
            .await
            .context("couldn't begin transaction")?;
        Ok(Self(transaction))
    }
}

impl ReqTransaction {
    pub(crate) async fn commit(self) -> anyhow::Result<()> {
        self.0
            .commit()
            .await
            .context("failed to commit transaction")
    }
}

impl Deref for ReqTransaction {
    type Target = Transaction<'static, Postgres>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ReqTransaction {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
