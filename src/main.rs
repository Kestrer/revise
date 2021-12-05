#![allow(clippy::single_component_path_imports)] // https://github.com/rust-lang/rust-clippy/issues/7923

use std::{net::SocketAddr, path::PathBuf, time::Duration};

use anyhow::Context as _;
use axum::{
    body::BoxBody,
    handler::Handler as _,
    http::{
        uri::{Authority, Scheme},
        HeaderMap, Request, Response, StatusCode, Uri,
    },
    response::{IntoResponse, Redirect},
    routing::get,
    AddExtensionLayer, Router,
};
use axum_server::tls_rustls::RustlsConfig;
use headers::{CacheControl, ContentType, HeaderMapExt as _};
use session::Session;
use sqlx::{
    migrate,
    migrate::MigrateDatabase as _,
    postgres::{PgPool, Postgres},
};
use structopt::StructOpt;
use tokio::{signal, task};
use tower::util::MapResponseLayer;
use utils::EndpointResult;

#[cfg_attr(debug_assertions, path = "dynamic_assets.rs")]
#[cfg_attr(not(debug_assertions), path = "static_assets.rs")]
mod assets;

mod accounts;
mod cards;
mod session;
mod utils;

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

async fn redirect_https(https_port: u16, headers: HeaderMap, uri: Uri) -> EndpointResult {
    let host = headers
        .get("host")
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "host header missing").into_response())?;

    let old_auth = Authority::try_from(host.as_bytes()).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("host header invalid: {}", e),
        )
            .into_response()
    })?;

    let new_auth = Authority::try_from(&*format!("{}:{}", old_auth.host(), https_port)).unwrap();

    let mut uri_parts = uri.into_parts();
    uri_parts.scheme = Some(Scheme::HTTPS);
    uri_parts.authority = Some(new_auth);
    let uri = Uri::try_from(uri_parts).context("failed to reconstruct HTTPS URI")?;

    Ok(Redirect::permanent(uri).into_response())
}

fn router(db: PgPool) -> Router {
    Router::new()
        .route("/", get(index))
        .nest("/accounts", accounts::routes())
        .nest("/cards", cards::routes())
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
