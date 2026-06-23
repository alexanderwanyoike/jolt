use std::path::Path;

use anyhow::{Context, Result};

use crate::client::DaemonClient;

const DEFAULT_API_PORT: u16 = 9862;

pub async fn export(out: &Path, passphrase: &str, label: Option<&str>) -> Result<()> {
    let client = DaemonClient::new(DEFAULT_API_PORT);
    let response = client.export_identity(passphrase, label).await?;
    let identity = response["identity"].as_str().unwrap_or("<unknown>");
    let bundle = response
        .get("bundle")
        .cloned()
        .context("daemon response did not include an identity bundle")?;
    let raw = serde_json::to_string_pretty(&bundle)?;
    std::fs::write(out, raw)
        .with_context(|| format!("failed to write identity bundle to {}", out.display()))?;
    println!("Exported identity {identity} to {}", out.display());
    println!("Anyone with this file and its passphrase can act as this identity.");
    Ok(())
}

pub async fn import(from: &Path, passphrase: &str, allow_overwrite: bool) -> Result<()> {
    let raw = std::fs::read_to_string(from)
        .with_context(|| format!("failed to read identity bundle from {}", from.display()))?;
    let bundle: serde_json::Value = serde_json::from_str(&raw)
        .with_context(|| format!("identity bundle {} is not valid JSON", from.display()))?;
    let client = DaemonClient::new(DEFAULT_API_PORT);
    let response = client
        .import_identity(bundle, passphrase, allow_overwrite)
        .await?;
    let identity = response["identity"].as_str().unwrap_or("<unknown>");
    println!("Imported identity {identity} from {}", from.display());
    if response["restart_required"].as_bool().unwrap_or(false) {
        println!("Restart the daemon before using the imported identity.");
    }
    println!("App sessions were not imported; approve apps again on this device.");
    Ok(())
}
