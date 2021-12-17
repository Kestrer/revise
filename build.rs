use std::{
    env,
    error::Error,
    fmt::{Display, Write as _},
    fs, io,
    path::Path,
    path::PathBuf,
    process::{Command, Stdio},
};

use md5::{Digest, Md5};

fn main() -> Result<(), Box<dyn Error>> {
    if env::var("PROFILE").unwrap() == "debug" {
        println!("cargo:rerun-if-changed=build.rs");
        return Ok(());
    }

    env::set_current_dir("html").context("failed to change to html dir")?;

    match fs::remove_dir_all("dist") {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(context(e, "failed to remove dist directory")),
    }

    let status = Command::new("npm")
        .arg("run")
        .arg("build-prod")
        .stdout(Stdio::null())
        .spawn()?
        .wait()?;
    if !status.success() {
        return Err("failed to build site with webpack".into());
    }

    let mut dist_dir = env::current_dir().context("failed to get current dir")?;
    dist_dir.push("dist");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());

    for asset in ["home.html", "dashboard.html"] {
        fs::write(
            out_dir.join(asset),
            mutable_asset_descriptor(dist_dir.join(asset))?,
        )
        .context("failed to write mutable asset descriptor")?;
    }

    fs::write(
        out_dir.join("immutable-assets"),
        immutable_assets_descriptor(dist_dir.join("assets"))?,
    )
    .context("failed to write immutable assets descriptor")?;

    println!("cargo:rerun-if-changed=html");

    Ok(())
}

fn mutable_asset_descriptor(path: impl AsRef<Path>) -> Result<String, Box<dyn Error>> {
    let file_contents = fs::read(&path)
        .with_context(|| format!("failed to read mutable asset {}", path.as_ref().display()))?;
    let file_hash = Md5::digest(&file_contents);

    Ok(format!(
        "crate::assets::MutableAsset {{\
            asset: crate::assets::Asset {{\
                uncompressed: include_bytes!(\"{path}\"),\
                brotli: {brotli},\
            }},\
            etag: \"\\\"{etag}\\\"\"\
        }}",
        path = path.as_ref().to_str().unwrap().escape_default(),
        brotli = brotli(&path),
        etag = base64::encode_config(&file_hash, base64::STANDARD_NO_PAD),
    ))
}

fn immutable_assets_descriptor(assets_dir: impl AsRef<Path>) -> Result<String, Box<dyn Error>> {
    let mut res = "[".to_owned();

    for entry in fs::read_dir(assets_dir).context("failed to read assets dir")? {
        let entry = entry.context("failed to get assets directory entry")?;
        let path = entry.path();

        let content_type = match path.extension().map(|s| s.to_str().unwrap()) {
            Some("js") => "text/javascript",
            Some("css") => "text/css",
            Some("br") => continue,
            _ => {
                return Err(format!("file {} has an unknown MIME type", path.display()).into());
            }
        };

        write!(
            res,
            "ImmutableAsset {{\
                path: \"/{file_name}\",\
                content_type: axum::http::HeaderValue::from_static(\"{content_type}\"),\
                asset: Asset {{\
                    uncompressed: include_bytes!(\"{path}\"),\
                    brotli: {brotli}\
                }}\
            }},",
            file_name = path.file_name().unwrap().to_str().unwrap().escape_default(),
            content_type = content_type,
            path = path.to_str().unwrap().escape_default(),
            brotli = brotli(&path),
        )
        .unwrap();
    }

    res.push(']');

    Ok(res)
}

fn brotli(path: impl AsRef<Path>) -> String {
    let brotli_path = format!("{}.br", path.as_ref().to_str().unwrap());
    if Path::new(&brotli_path).exists() {
        format!("Some(include_bytes!(\"{}\"))", brotli_path.escape_default())
    } else {
        "None".to_owned()
    }
}

trait Context: Sized {
    type Output;
    fn context<C: Display>(self, msg: C) -> Result<Self::Output, Box<dyn Error>> {
        self.with_context(|| msg)
    }
    fn with_context<C, F>(self, f: F) -> Result<Self::Output, Box<dyn Error>>
    where
        C: Display,
        F: FnOnce() -> C;
}

impl<T, E: Error> Context for Result<T, E> {
    type Output = T;
    fn with_context<C, F>(self, f: F) -> Result<Self::Output, Box<dyn Error>>
    where
        C: Display,
        F: FnOnce() -> C,
    {
        self.map_err(|e| context(e, &f().to_string()))
    }
}

fn context<E: Error>(error: E, context: &str) -> Box<dyn Error> {
    format!("{}: {}", context, error).into()
}
