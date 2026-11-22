use anyhow::{Context, Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use minisign::{PublicKey, SecretKey, SecretKeyBox};
use serde::Deserialize;
use std::{
    env, fs,
    io::BufReader,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    version: String,
    updater_public_key: String,
}

fn main() -> Result<()> {
    let files: Vec<_> = env::args_os().skip(1).map(PathBuf::from).collect();
    ensure!(!files.is_empty(), "Specify the installer files to sign");
    let config: Config = serde_json::from_str(include_str!("../../app.json"))?;
    let public_key = PublicKey::from_box(
        String::from_utf8(STANDARD.decode(config.updater_public_key)?)?.into(),
    )?;
    let encoded_key = env::var("S3_STUDIO_UPDATE_KEY").context("Missing update signing key")?;
    let secret_key = String::from_utf8(STANDARD.decode(encoded_key.trim())?)
        .context("Update signing key is not UTF-8")?;
    let secret_key = SecretKey::from_box(
        SecretKeyBox::from_string(&secret_key)?,
        Some(env::var("S3_STUDIO_UPDATE_KEY_PASSWORD").unwrap_or_default()),
    )?;
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    for path in files {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .context("Invalid installer filename")?;
        ensure!(
            !name.contains(['\t', '\r', '\n']),
            "Invalid installer filename"
        );
        let trusted = format!(
            "timestamp:{timestamp}\tfile:{name}\tversion:{}",
            config.version
        );
        let signature = minisign::sign(
            Some(&public_key),
            &secret_key,
            BufReader::new(fs::File::open(&path)?),
            Some(&trusted),
            None,
        )?;
        fs::write(
            path.with_added_extension("sig"),
            STANDARD.encode(signature.to_string()) + "\n",
        )?;
        println!("Signed {name} for S3 Studio {}", config.version);
    }
    Ok(())
}
