#![allow(clippy::single_component_path_imports)] // https://github.com/rust-lang/rust-clippy/issues/7923

use std::{net::SocketAddr, path::PathBuf, time::Duration};

use anyhow::Context as _;
use axum::{
    async_trait,
    body::{self, BoxBody, Bytes, HttpBody},
    extract::{Extension, Form, FromRequest, RequestParts},
    handler::Handler as _,
    http::{
        header,
        uri::{Authority, Scheme},
        HeaderMap, HeaderValue, Request, Response, StatusCode, Uri,
    },
    response::{IntoResponse, Redirect},
    routing::handler_method_routing::{get, post},
    AddExtensionLayer, Json, Router,
};
use axum_server::tls_rustls::RustlsConfig;
use headers::{CacheControl, ContentType, HeaderMapExt as _};
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use sqlx::{
    migrate,
    migrate::MigrateDatabase as _,
    postgres::{PgPool, Postgres},
};
use structopt::StructOpt;
use tokio::{signal, task};
use tower::util::MapResponseLayer;

#[cfg_attr(debug_assertions, path = "dynamic_assets.rs")]
#[cfg_attr(not(debug_assertions), path = "static_assets.rs")]
mod assets;

#[derive(StructOpt)]
struct Opts {
    /// Drop the database before running.
    #[structopt(long)]
    drop: bool,

    /// The URL to the Postgres database server to connect to, for example
    /// <postgres:///revise?host=/tmp&user=postgres>.
    #[structopt(long, env = "DATABASE_URL")]
    database: String,

    /// The TLS certificate file to use, in PEM format.
    #[structopt(long)]
    tls_cert: PathBuf,

    /// The TLS key file to use, in PEM format.
    #[structopt(long)]
    tls_key: PathBuf,

    /// The port to run the HTTP web server on.
    #[structopt(long)]
    http_port: u16,

    /// The port to run the HTTPS web server on.
    #[structopt(long)]
    https_port: u16,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::formatted_builder()
        .filter(Some("revise"), log::LevelFilter::Info)
        .init();

    let opts = Opts::from_args();

    let tls_config = RustlsConfig::from_pem_file(&opts.tls_cert, &opts.tls_key)
        .await
        .context("failed to configure TLS")?;

    let mut exists = Postgres::database_exists(&opts.database)
        .await
        .context("failed to check database existence")?;

    if exists && opts.drop {
        log::info!("dropping database");
        Postgres::drop_database(&opts.database)
            .await
            .context("could not drop database")?;
        exists = false;
    }

    if !exists {
        log::info!("database does not exist; creating it now");
        Postgres::create_database(&opts.database)
            .await
            .context("could not create database")?;
    }

    let db = PgPool::connect(&opts.database)
        .await
        .context("failed to open database")?;

    migrate!()
        .run(&db)
        .await
        .context("database migration failed")?;
    log::info!("ran migrations");

    let asset_manager = assets::AssetManager::new()?;

    let http_handle = axum_server::Handle::new();
    let http_server = axum_server::bind(SocketAddr::from(([0, 0, 0, 0], opts.http_port)))
        .handle(http_handle.clone());

    let https_handle = axum_server::Handle::new();
    let https_server = axum_server::bind_rustls(
        SocketAddr::from(([0, 0, 0, 0], opts.https_port)),
        tls_config,
    )
    .handle(https_handle.clone());

    let http_task = task::spawn(async move {
        http_server
            .serve(
                (move |headers, uri| redirect_https(opts.https_port, headers, uri))
                    .into_make_service(),
            )
            .await
    });

    let https_task =
        task::spawn(async move { https_server.serve(router(db).into_make_service()).await });

    let listening_http = http_handle.listening().await;
    log::info!("HTTP server listening on {}", listening_http);

    let listening_https = https_handle.listening().await;
    log::info!("HTTPS server listening on {}", listening_https);

    signal::ctrl_c()
        .await
        .context("could not listen for CTRL+C")?;

    log::info!("shutting down");

    http_handle.shutdown();
    https_handle.shutdown();
    asset_manager.stop().await;
    http_task
        .await
        .unwrap()
        .context("could not start HTTP server")?;
    https_task
        .await
        .unwrap()
        .context("could not start HTTPS server")?;

    Ok(())
}

struct EndpointError(Response<BoxBody>);

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
    type Body = BoxBody;
    type BodyError = <Self::Body as HttpBody>::Error;
    fn into_response(self) -> Response<Self::Body> {
        self.0
    }
}

type EndpointResult = Result<Response<BoxBody>, EndpointError>;

async fn redirect_https(https_port: u16, headers: HeaderMap, uri: Uri) -> EndpointResult {
    let host = headers
        .get("host")
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Host header missing").into_response())?;

    let old_auth = Authority::try_from(host.as_bytes()).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("Host header invalid: {}", e),
        )
            .into_response_boxed()
    })?;

    let new_auth = Authority::try_from(&*format!("{}:{}", old_auth.host(), https_port)).unwrap();

    let mut uri_parts = uri.into_parts();
    uri_parts.scheme = Some(Scheme::HTTPS);
    uri_parts.authority = Some(new_auth);
    let uri = Uri::try_from(uri_parts).context("failed to reconstruct HTTPS URI")?;

    Ok(Redirect::permanent(uri).into_response_boxed())
}

fn router(db: PgPool) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/create-account", post(create_account))
        .route("/delete-account", post(delete_account))
        .route("/cards", get(cards).post(create_card))
        .layer(MapResponseLayer::new(|mut res: Response<BoxBody>| {
            res.headers_mut()
                .typed_insert(CacheControl::new().with_private().with_no_cache());
            res
        }))
        .nest(
            "/assets",
            assets::immutable_assets().layer(MapResponseLayer::new(|mut res: Response<_>| {
                if res.status().as_u16() < 400 {
                    res.headers_mut().typed_insert(
                        CacheControl::new()
                            .with_public()
                            .with_max_age(Duration::from_secs(60 * 60 * 24 * 365)),
                    );
                }
                res
            })),
        )
        .layer(AddExtensionLayer::new(db))
}

async fn index<B>(session: Option<Session>, req: Request<B>) -> EndpointResult {
    if session.is_some() {
        dashboard(req).await
    } else {
        home(req).await
    }
}

async fn home<B>(req: Request<B>) -> EndpointResult {
    let mut res = assets::mutable_asset!("home.html").call(req).await?;
    res.headers_mut().typed_insert(ContentType::html());
    Ok(res)
}

async fn dashboard<B>(req: Request<B>) -> EndpointResult {
    let mut res = assets::mutable_asset!("dashboard.html").call(req).await?;
    res.headers_mut().typed_insert(ContentType::html());
    Ok(res)
}

#[derive(Deserialize)]
struct LogIn {
    email: String,
    password: String,
}

async fn login(db: Extension<PgPool>, form: Form<LogIn>) -> EndpointResult {
    let mut transaction = db.begin().await.context("couldn't begin transaction")?;

    let user_id: i64 = sqlx::query_scalar(
        "SELECT id FROM users WHERE email = $1 AND password = crypt($2, password)",
    )
    .bind(&form.email)
    .bind(&form.password)
    .fetch_optional(&mut transaction)
    .await
    .context("failed to check password correctness")?
    .ok_or_else(|| Redirect::to(Uri::from_static("/?loginError=")).into_response_boxed())?;

    let session = Session::new();

    sqlx::query("INSERT INTO session_cookies VALUES ($1, $2)")
        .bind(&session.0)
        .bind(&user_id)
        .execute(&mut transaction)
        .await
        .context("failed to insert new session cookie")?;

    transaction
        .commit()
        .await
        .context("failed to commit transaction")?;

    Ok(session.set_cookie_on(Redirect::to(Uri::from_static("/"))))
}

async fn logout(db: Extension<PgPool>, session: Session) -> EndpointResult {
    sqlx::query("DELETE FROM session_cookies WHERE cookie_value = $1")
        .bind(&session.0)
        .execute(&*db)
        .await
        .context("failed to log out")?;

    Ok(session.clear_cookie_on(Redirect::to(Uri::from_static("/"))))
}

#[derive(Deserialize)]
struct CreateAccount {
    email: String,
    password: String,
}

async fn create_account(db: Extension<PgPool>, form: Form<CreateAccount>) -> EndpointResult {
    if form.email.is_empty() || form.password.is_empty() {
        return Err(EndpointError(StatusCode::BAD_REQUEST.into_response_boxed()));
    }

    let mut transaction = db.begin().await.context("couldn't begin transaction")?;

    let user_id: i64 = sqlx::query_scalar(
        "\
            INSERT INTO users VALUES (DEFAULT, $1, crypt($2, gen_salt('bf', 8))) \
            ON CONFLICT DO NOTHING \
            RETURNING id\
        ",
    )
    .bind(&form.email)
    .bind(&form.password)
    .fetch_optional(&mut transaction)
    .await
    .context("failed to add new user")?
    .ok_or_else(|| Redirect::to(Uri::from_static("/?createAccountError=")).into_response_boxed())?;

    let session = Session::new();

    sqlx::query("INSERT INTO session_cookies VALUES ($1, $2)")
        .bind(&session.0)
        .bind(&user_id)
        .execute(&mut transaction)
        .await
        .context("failed to insert new session cookie")?;

    transaction
        .commit()
        .await
        .context("failed to commit transaction")?;

    Ok(session.set_cookie_on(Redirect::to(Uri::from_static("/"))))
}

async fn delete_account(db: Extension<PgPool>, session: Session) -> EndpointResult {
    sqlx::query(
        "\
            DELETE FROM users \
            WHERE id = (SELECT for_user FROM session_cookies WHERE cookie_value = $1)\
        ",
    )
    .bind(&session.0)
    .execute(&*db)
    .await
    .context("failed to delete account")?;

    Ok(session.clear_cookie_on(Redirect::to(Uri::from_static("/"))))
}

#[derive(sqlx::FromRow, Serialize)]
struct Card {
    id: i64,
    terms: String,
    definitions: String,
    case_sensitive: bool,
    knowledge: i16,
    safety_net: bool,
}

async fn cards(db: Extension<PgPool>, session: Session) -> EndpointResult {
    let cards: Vec<Card> = sqlx::query_as(
        "\
            SELECT id,terms,definitions,case_sensitive,knowledge,safety_net FROM cards \
            WHERE owner = (SELECT for_user FROM session_cookies WHERE cookie_value = $1)\
        ",
    )
    .bind(&session.0)
    .fetch_all(&*db)
    .await
    .context("failed to query cards")?;

    Ok(Json(cards).into_response_boxed())
}

#[derive(Deserialize)]
struct CreateCard {
    terms: String,
    definitions: String,
    case_sensitive: bool,
}

async fn create_card(
    db: Extension<PgPool>,
    session: Session,
    body: Json<CreateCard>,
) -> EndpointResult {
    if body.terms.chars().all(|c| c == '\n') || body.definitions.chars().all(|c| c == '\n') {
        return Err(EndpointError(StatusCode::BAD_REQUEST.into_response_boxed()));
    }

    sqlx::query(
        "\
            INSERT INTO cards \
            VALUES (DEFAULT, (SELECT for_user FROM session_cookies WHERE cookie_value = $1), $2, $3, $4)\
        "
    )
    .bind(&session.0)
    .bind(&body.terms)
    .bind(&body.definitions)
    .bind(&body.case_sensitive)
    .execute(&*db)
    .await
    .context("failed to insert card")?;

    Ok(StatusCode::CREATED.into_response_boxed())
}

struct Session(String);

impl Session {
    fn new() -> Self {
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

    fn set_cookie_on(&self, response: impl IntoResponse) -> Response<BoxBody> {
        let mut response = response.into_response_boxed();
        response.headers_mut().insert(
            header::SET_COOKIE,
            HeaderValue::from_str(&format!(
                "session={};Max-Age={};Secure;HttpOnly",
                &self.0,
                60 * 60 * 24 * 365
            ))
            .unwrap(),
        );
        response
    }

    fn clear_cookie_on(&self, response: impl IntoResponse) -> Response<BoxBody> {
        let mut response = response.into_response_boxed();
        response.headers_mut().insert(
            header::SET_COOKIE,
            HeaderValue::from_static("session=;Max-Age=0"),
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

struct SessionRejection;

impl IntoResponse for SessionRejection {
    type Body = BoxBody;
    type BodyError = <BoxBody as HttpBody>::Error;
    fn into_response(self) -> Response<Self::Body> {
        (StatusCode::UNAUTHORIZED, "You are not logged in.").into_response_boxed()
    }
}

trait IntoResponseBoxed {
    fn into_response_boxed(self) -> Response<BoxBody>;
}
impl<T: IntoResponse> IntoResponseBoxed for T {
    fn into_response_boxed(self) -> Response<BoxBody> {
        self.into_response().map(body::boxed)
    }
}
