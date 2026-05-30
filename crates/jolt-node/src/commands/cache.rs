use anyhow::Result;

use jolt_store::{CacheConfig, ContentStore};

use crate::client::DaemonClient;
use crate::config::NodeConfig;
use crate::daemon;

/// Try to get a daemon client. Returns None if daemon is not running.
fn try_daemon_client() -> Option<DaemonClient> {
    let config = NodeConfig::default_dirs();
    let info = daemon::read_daemon_info(&config)?;
    if daemon::is_daemon_running(&config) {
        Some(DaemonClient::new(info.port))
    } else {
        None
    }
}

fn open_store() -> Result<ContentStore> {
    let config = NodeConfig::default_dirs();
    config.ensure_dirs()?;
    let store = ContentStore::open(&config.content_store_dir, CacheConfig::default())?;
    Ok(store)
}

pub async fn stats() -> Result<()> {
    if let Some(client) = try_daemon_client() {
        // Use daemon API
        let stats = client.cache_stats().await?;
        let total_cached = stats["total_cached"].as_u64().unwrap_or(0);
        let max_size = stats["max_size"].as_u64().unwrap_or(0);
        let cached_items = stats["cached_items"].as_u64().unwrap_or(0);
        let pinned_items = stats["pinned_items"].as_u64().unwrap_or(0);
        let pinned_size = stats["pinned_size"].as_u64().unwrap_or(0);
        let published_items = stats["published_items"].as_u64().unwrap_or(0);
        let total_published = stats["total_published"].as_u64().unwrap_or(0);
        let available = stats["available"].as_u64().unwrap_or(0);

        println!("Cache Statistics:");
        println!("  Cached items:    {cached_items}");
        println!(
            "  Cache size:      {} / {} ({:.1}%)",
            format_bytes(total_cached),
            format_bytes(max_size),
            if max_size > 0 {
                (total_cached as f64 / max_size as f64) * 100.0
            } else {
                0.0
            }
        );
        println!(
            "  Pinned items:    {} ({})",
            pinned_items,
            format_bytes(pinned_size)
        );
        println!(
            "  Published items: {} ({})",
            published_items,
            format_bytes(total_published)
        );
        println!("  Available:       {}", format_bytes(available));
    } else {
        // Direct store access fallback
        let store = open_store()?;
        let s = store.stats();
        println!("Cache Statistics:");
        println!("  Cached items:    {}", s.cached_items);
        println!(
            "  Cache size:      {} / {} ({:.1}%)",
            format_bytes(s.total_cached),
            format_bytes(s.max_size),
            if s.max_size > 0 {
                (s.total_cached as f64 / s.max_size as f64) * 100.0
            } else {
                0.0
            }
        );
        println!(
            "  Pinned items:    {} ({})",
            s.pinned_items,
            format_bytes(s.pinned_size)
        );
        println!(
            "  Published items: {} ({})",
            s.published_items,
            format_bytes(s.total_published)
        );
        println!("  Available:       {}", format_bytes(s.available));
    }

    Ok(())
}

pub async fn list() -> Result<()> {
    if let Some(client) = try_daemon_client() {
        let entries = client.cache_entries().await?;
        let empty = vec![];
        let entries = entries.as_array().unwrap_or(&empty);

        if entries.is_empty() {
            println!("No cached content.");
            return Ok(());
        }

        println!(
            "{:<64}  {:>10}  {:>6}  {:>20}",
            "Content ID", "Size", "Pinned", "Last Accessed"
        );
        println!("{}", "-".repeat(106));

        for entry in entries {
            let id = entry["content_id"].as_str().unwrap_or("?");
            let size = entry["size"].as_u64().unwrap_or(0);
            let pinned = if entry["pinned"].as_bool().unwrap_or(false) {
                "yes"
            } else {
                "no"
            };
            let last_accessed = entry["last_accessed"].as_u64().unwrap_or(0);

            let id_display = if id.len() > 62 {
                format!("{}...", &id[..60])
            } else {
                id.to_string()
            };

            println!(
                "{:<64}  {:>10}  {:>6}  {:>20}",
                id_display,
                format_bytes(size),
                pinned,
                format_time_ago(last_accessed)
            );
        }
    } else {
        let store = open_store()?;
        let entries = store.list_entries();

        if entries.is_empty() {
            println!("No cached content.");
            return Ok(());
        }

        println!(
            "{:<64}  {:>10}  {:>6}  {:>20}",
            "Content ID", "Size", "Pinned", "Last Accessed"
        );
        println!("{}", "-".repeat(106));

        let mut sorted: Vec<_> = entries;
        sorted.sort_by(|a, b| b.last_accessed.cmp(&a.last_accessed));

        for entry in sorted {
            let pinned = if entry.pinned { "yes" } else { "no" };
            let id_display = if entry.content_id.len() > 62 {
                format!("{}...", &entry.content_id[..60])
            } else {
                entry.content_id.clone()
            };

            println!(
                "{:<64}  {:>10}  {:>6}  {:>20}",
                id_display,
                format_bytes(entry.size),
                pinned,
                format_time_ago(entry.last_accessed)
            );
        }
    }

    Ok(())
}

pub async fn pin(content_id: &str) -> Result<()> {
    if let Some(client) = try_daemon_client() {
        client.pin(content_id).await?;
    } else {
        let mut store = open_store()?;
        store.pin(content_id)?;
    }
    println!("Pinned: {content_id}");
    Ok(())
}

pub async fn unpin(content_id: &str) -> Result<()> {
    if let Some(client) = try_daemon_client() {
        client.unpin(content_id).await?;
    } else {
        let mut store = open_store()?;
        store.unpin(content_id)?;
    }
    println!("Unpinned: {content_id}");
    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn format_time_ago(unix_secs: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if unix_secs == 0 || unix_secs > now {
        return "just now".to_string();
    }

    let diff = now - unix_secs;
    if diff < 60 {
        format!("{diff}s ago")
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}
