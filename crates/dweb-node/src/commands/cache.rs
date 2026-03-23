use anyhow::Result;

use dweb_store::{CacheConfig, ContentStore};

use crate::config::NodeConfig;

fn open_store() -> Result<ContentStore> {
    let config = NodeConfig::default_dirs();
    config.ensure_dirs()?;
    let store = ContentStore::open(&config.content_store_dir, CacheConfig::default())?;
    Ok(store)
}

pub fn stats() -> Result<()> {
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
    println!("  Published items: {} ({})", s.published_items, format_bytes(s.total_published));
    println!("  Available:       {}", format_bytes(s.available));

    Ok(())
}

pub fn list() -> Result<()> {
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
        let ago = format_time_ago(entry.last_accessed);

        // Truncate content_id for display if too long
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
            ago
        );
    }

    Ok(())
}

pub fn pin(content_id: &str) -> Result<()> {
    let mut store = open_store()?;
    store.pin(content_id)?;
    println!("Pinned: {content_id}");
    Ok(())
}

pub fn unpin(content_id: &str) -> Result<()> {
    let mut store = open_store()?;
    store.unpin(content_id)?;
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
