//! Implementation of asset serving based on static files included in the binary. Used in release
//! mode.

use axum::{
    http::{
        header::{self, HeaderMap, HeaderValue},
        Request, StatusCode,
    },
    response::{IntoResponse, Response},
    routing, Router,
};
use headers::{ETag, HeaderMapExt as _, IfNoneMatch};

use crate::utils::EndpointResult;

// In development mode, this would start a subprocess running `npm run watch` - in release mode, it
// is a no-op.
pub(crate) struct AssetManager;
impl AssetManager {
    pub(crate) fn new() -> anyhow::Result<Self> {
        Ok(Self)
    }
    pub(crate) async fn stop(self) {}
}

#[derive(Clone)]
pub(crate) struct Asset {
    pub(crate) uncompressed: &'static [u8],
    pub(crate) brotli: Option<&'static [u8]>,
}

impl Asset {
    fn response(&self, headers: &HeaderMap) -> Response {
        let brotli = self.brotli.filter(|_| {
            headers
                .get(header::ACCEPT_ENCODING)
                .and_then(|header| header.to_str().ok())
                .map_or(false, supports_brotli)
        });
        let mut res = brotli.unwrap_or(self.uncompressed).into_response();
        if brotli.is_some() {
            res.headers_mut()
                .insert("content-encoding", HeaderValue::from_static("br"));
        }
        res
    }
}

pub(crate) struct MutableAsset {
    pub(crate) asset: Asset,
    pub(crate) etag: &'static str,
}

impl MutableAsset {
    pub(crate) async fn call<B>(self, req: Request<B>) -> EndpointResult {
        let etag: ETag = self.etag.parse().unwrap();
        let mut res = if req
            .headers()
            .typed_get::<IfNoneMatch>()
            .map_or(false, |h| !h.precondition_passes(&etag))
        {
            StatusCode::NOT_MODIFIED.into_response()
        } else {
            self.asset.response(req.headers())
        };
        res.headers_mut().typed_insert(etag);
        Ok(res)
    }
}

macro_rules! mutable_asset {
    ($file:literal) => {
        include!(concat!(env!("OUT_DIR"), "/", $file))
    };
}
pub(crate) use mutable_asset;

struct ImmutableAsset {
    path: &'static str,
    content_type: HeaderValue,
    asset: Asset,
}

pub(crate) fn immutable_assets() -> Router {
    let mut router = Router::new();
    for asset in include!(concat!(env!("OUT_DIR"), "/immutable-assets")) {
        router = router.route(
            asset.path,
            routing::get(move |headers: HeaderMap| async move {
                let mut res = asset.asset.response(&headers);
                res.headers_mut().insert("content-type", asset.content_type);
                res
            }),
        );
    }
    router
}

fn supports_brotli(header: &str) -> bool {
    header
        .split(',')
        .any(|s| s.trim().splitn(2, ";q=").next().unwrap() == "br")
}
