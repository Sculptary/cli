//! Release uploads hash and transfer a file in bounded memory.
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::api::Client;

#[derive(Subcommand)]
pub enum ReleaseCmd {
    /// Attach a file to an existing release, verified with SHA-256.
    Upload {
        tag: String,
        file: PathBuf,
        /// Repository (defaults to the current Git origin).
        #[arg(long, value_parser = crate::commands::full_name)]
        repo: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Deserialize)]
struct Release {
    id: String,
    asset_upload_max_bytes: Option<u64>,
}
#[derive(Deserialize)]
struct Prepared {
    id: String,
    upload: Transfer,
}
#[derive(Deserialize)]
struct Transfer {
    href: String,
    headers: BTreeMap<String, String>,
}

fn transfer(prepared: &Prepared, file: File, size: u64) -> Result<()> {
    // A separate client has no API bearer token, cookie store or redirects.
    let http = reqwest::blocking::Client::builder()
        .user_agent(concat!("shape-cli/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30 * 60))
        .build()?;
    let mut request = http.put(&prepared.upload.href);
    for (name, value) in &prepared.upload.headers {
        if name.eq_ignore_ascii_case("authorization") || name.eq_ignore_ascii_case("cookie") {
            bail!("storage upload returned an unexpected credential header");
        }
        request = request.header(name, value);
    }
    let response = request
        .body(reqwest::blocking::Body::sized(file, size))
        .send()
        .map_err(|error| error.without_url())
        .context("storage upload failed")?;
    if !response.status().is_success() {
        bail!("storage upload failed with status {}", response.status());
    }
    Ok(())
}

pub fn upload(api: &Client, repo: &str, tag: &str, path: &Path, as_json: bool) -> Result<()> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("file name must be valid UTF-8")?;
    if name
        .chars()
        .any(|c| c.is_control() || c == '/' || c == '\\')
    {
        bail!("file name cannot contain folders or control characters");
    }
    let mut file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        bail!("upload path must be a regular file");
    }
    let size = metadata.len();
    let release: Release =
        api.get_query(&format!("/repos/{repo}/releases/by-tag"), &[("tag", tag)])?;
    let limit = release
        .asset_upload_max_bytes
        .context("you do not have permission to upload release files")?;
    if size > limit {
        bail!("file exceeds the maximum upload size ({limit} bytes)");
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut read = 0_u64;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        read += count as u64;
        if read > size {
            bail!("file changed while calculating its checksum; try again");
        }
        hasher.update(&buffer[..count]);
    }
    if read != size {
        bail!("file changed while calculating its checksum; try again");
    }
    let sha256: String = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    file.rewind()?;
    let root = format!("/repos/{repo}/releases/{}/assets", release.id);
    let prepared: Prepared =
        api.post(&root, &json!({"name":name, "size":size, "sha256":sha256}))?;
    let endpoint = format!("{root}/{}", prepared.id);
    let mut verification_started = false;
    let result = (|| -> Result<Value> {
        transfer(&prepared, file, size)?;
        verification_started = true;
        api.post(&format!("{endpoint}/complete"), &json!({})).context("Upload finished, but verification could not be confirmed. Check the release before trying again.")
    })();
    match result {
        Ok(asset) => {
            if as_json {
                println!("{}", serde_json::to_string_pretty(&asset)?);
            } else {
                println!("Attached {name} ({size} bytes).\nSHA-256: {sha256}");
            }
            Ok(())
        }
        Err(error) => {
            if !verification_started && api.delete(&endpoint).is_err() {
                eprintln!("The pending upload could not be removed. It will expire automatically.");
            }
            Err(error)
        }
    }
}
