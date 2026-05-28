use std::path::Path;

use anyhow::{Context, Result};

/// HTTP client for CLI commands to communicate with the running daemon.
pub struct DaemonClient {
    base_url: String,
    client: reqwest::Client,
}

impl DaemonClient {
    pub fn new(port: u16) -> Self {
        Self {
            base_url: format!("http://127.0.0.1:{port}"),
            client: reqwest::Client::new(),
        }
    }

    pub async fn health(&self) -> Result<bool> {
        let resp = self
            .client
            .get(format!("{}/api/v1/health", self.base_url))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await?;
        Ok(resp.status().is_success())
    }

    pub async fn status(&self) -> Result<serde_json::Value> {
        let resp = self
            .client
            .get(format!("{}/api/v1/status", self.base_url))
            .send()
            .await
            .context("Failed to connect to daemon")?;
        let body = resp.json().await?;
        Ok(body)
    }

    pub async fn publish(&self, file_path: &Path, path: Option<&str>) -> Result<serde_json::Value> {
        let data = std::fs::read(file_path)
            .with_context(|| format!("Failed to read file: {}", file_path.display()))?;

        let file_name = file_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_string());

        let part = reqwest::multipart::Part::bytes(data).file_name(file_name);
        let mut form = reqwest::multipart::Form::new().part("file", part);
        if let Some(path) = path {
            form = form.text("path", path.to_string());
        }

        let resp = self
            .client
            .post(format!("{}/api/v1/publish", self.base_url))
            .multipart(form)
            .send()
            .await
            .context("Failed to connect to daemon")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Publish failed ({status}): {body}");
        }

        let body = resp.json().await?;
        Ok(body)
    }

    pub async fn fetch(&self, target: &str) -> Result<serde_json::Value> {
        let resp = self
            .client
            .post(format!("{}/api/v1/fetch", self.base_url))
            .json(&serde_json::json!({ "target": target }))
            .send()
            .await
            .context("Failed to connect to daemon")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Fetch failed ({status}): {body}");
        }

        let body = resp.json().await?;
        Ok(body)
    }

    pub async fn resolve(&self, address: &str) -> Result<serde_json::Value> {
        let resp = self
            .client
            .post(format!("{}/api/v1/resolve", self.base_url))
            .json(&serde_json::json!({ "address": address }))
            .send()
            .await
            .context("Failed to connect to daemon")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Resolve failed ({status}): {body}");
        }

        let body = resp.json().await?;
        Ok(body)
    }

    pub async fn cache_stats(&self) -> Result<serde_json::Value> {
        let resp = self
            .client
            .get(format!("{}/api/v1/cache/stats", self.base_url))
            .send()
            .await
            .context("Failed to connect to daemon")?;
        let body = resp.json().await?;
        Ok(body)
    }

    pub async fn cache_entries(&self) -> Result<serde_json::Value> {
        let resp = self
            .client
            .get(format!("{}/api/v1/cache/entries", self.base_url))
            .send()
            .await
            .context("Failed to connect to daemon")?;
        let body = resp.json().await?;
        Ok(body)
    }

    pub async fn pin(&self, content_id: &str) -> Result<()> {
        let resp = self
            .client
            .post(format!("{}/api/v1/cache/pin/{content_id}", self.base_url))
            .send()
            .await
            .context("Failed to connect to daemon")?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Pin failed: {body}");
        }
        Ok(())
    }

    pub async fn unpin(&self, content_id: &str) -> Result<()> {
        let resp = self
            .client
            .delete(format!("{}/api/v1/cache/pin/{content_id}", self.base_url))
            .send()
            .await
            .context("Failed to connect to daemon")?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Unpin failed: {body}");
        }
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<()> {
        // Shutdown is sent by dropping the daemon handle or via signal.
        // For now, we don't have a shutdown endpoint exposed.
        // The CLI `stop` command will use the PID to send SIGTERM.
        Ok(())
    }
}
