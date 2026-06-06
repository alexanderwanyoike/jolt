use anyhow::Result;
use serde_json::Value;

use crate::client::DaemonClient;
use crate::config::NodeConfig;
use crate::daemon;

pub async fn status(json: bool) -> Result<()> {
    let config = NodeConfig::default_dirs();

    match daemon::find_single_running_daemon(&config) {
        Ok(Some(info)) => {
            if daemon::read_daemon_info(&config).is_some_and(|stored| stored.pid == info.pid)
                && !daemon::is_daemon_running(&config)
            {
                daemon::clear_daemon_info(&config);
                println!("Daemon is not running (stale PID file cleaned up)");
                return Ok(());
            }

            let client = DaemonClient::new(info.port);
            let status = client.relay_status().await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else {
                print!("{}", format_relay_status(&status, Some(info.port)));
            }
        }
        Ok(None) => {
            if daemon::read_daemon_info(&config).is_some() {
                daemon::clear_daemon_info(&config);
                println!("Jolt is not running (stale PID file cleaned up)");
            } else {
                println!("Jolt is not running. Start with: jolt start");
            }
        }
        Err(daemons) => {
            println!("Multiple Jolt daemons are running:");
            for daemon in daemons {
                println!("  PID {} on API port {}", daemon.pid, daemon.port);
            }
            println!("Run relay status with the same XDG_DATA_HOME as the target daemon.");
        }
    }

    Ok(())
}

pub async fn diagnose_identity(identity: &str, json: bool) -> Result<()> {
    let config = NodeConfig::default_dirs();

    match daemon::find_single_running_daemon(&config) {
        Ok(Some(info)) => {
            if daemon::read_daemon_info(&config).is_some_and(|stored| stored.pid == info.pid)
                && !daemon::is_daemon_running(&config)
            {
                daemon::clear_daemon_info(&config);
                println!("Daemon is not running (stale PID file cleaned up)");
                return Ok(());
            }

            let client = DaemonClient::new(info.port);
            let diagnosis = client.relay_diagnose_identity(identity).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&diagnosis)?);
            } else {
                print!("{}", format_identity_diagnosis(&diagnosis));
            }
        }
        Ok(None) => {
            if daemon::read_daemon_info(&config).is_some() {
                daemon::clear_daemon_info(&config);
                println!("Jolt is not running (stale PID file cleaned up)");
            } else {
                println!("Jolt is not running. Start with: jolt start");
            }
        }
        Err(daemons) => {
            println!("Multiple Jolt daemons are running:");
            for daemon in daemons {
                println!("  PID {} on API port {}", daemon.pid, daemon.port);
            }
            println!("Run relay diagnose with the same XDG_DATA_HOME as the target daemon.");
        }
    }

    Ok(())
}

fn format_relay_status(status: &Value, api_port: Option<u16>) -> String {
    let relay = if bool_field(status, "relay_enabled") {
        "enabled"
    } else {
        "disabled"
    };
    let bootstrap = object_field(status, "bootstrap");
    let peers = object_field(status, "peers");
    let cache = object_field(status, "cache");

    let mut output = String::new();
    output.push_str(&format!("Relay: {relay}\n"));
    output.push_str(&format!(
        "Peer ID: {}\n",
        string_field(status, "peer_id", "unknown")
    ));
    output.push_str(&format!(
        "Jolt: {}\n",
        string_field(status, "identity_address", "unknown")
    ));
    if let Some(port) = api_port {
        output.push_str(&format!("API port: {port}\n"));
    }
    output.push_str(&format!(
        "Bootstrap: {} ({} connected / {} effective / {} configured)\n",
        string_field(bootstrap, "state", "unknown"),
        number_field(bootstrap, "connected_peer_count"),
        number_field(bootstrap, "effective_relay_count"),
        number_field(bootstrap, "configured_relay_count")
    ));
    if let Some(error) = bootstrap.get("last_error").and_then(Value::as_str) {
        output.push_str(&format!("Bootstrap error: {error}\n"));
    }
    output.push_str(&format!(
        "Known relays: {}\n",
        number_field(status, "known_relay_count")
    ));
    output.push_str(&format!(
        "Peers: {} connected ({} direct / {} relayed)\n",
        number_field(peers, "connected"),
        number_field(peers, "direct"),
        number_field(peers, "relayed")
    ));
    output.push_str(&format!(
        "Pins: {} items / {}\n",
        number_field(cache, "pinned_items"),
        format_bytes(number_field(cache, "pinned_bytes"))
    ));
    output.push_str(&format!(
        "Cache: {} items / {}\n",
        number_field(cache, "cached_items"),
        format_bytes(number_field(cache, "cached_bytes"))
    ));

    if status
        .get("relay_record")
        .is_some_and(|value| !value.is_null())
    {
        output.push_str("Relay record: present\n");
    } else {
        output.push_str("Relay record: none\n");
    }

    if let Some(home_relay) = status.get("home_relay").and_then(Value::as_object) {
        let multiaddr = home_relay
            .get("multiaddr")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        output.push_str(&format!("Home relay: {multiaddr}\n"));
    } else {
        output.push_str("Home relay: not configured\n");
    }

    if let Some(addrs) = status.get("listen_addresses").and_then(Value::as_array) {
        if !addrs.is_empty() {
            output.push_str("Listening:\n");
            for addr in addrs {
                if let Some(addr) = addr.as_str() {
                    output.push_str(&format!("  {addr}\n"));
                }
            }
        }
    }

    output
}

fn format_identity_diagnosis(diagnosis: &Value) -> String {
    let cache = object_field(diagnosis, "local_update_log_cache");
    let hint = object_field(diagnosis, "identity_head_hint");
    let forwarding = object_field(diagnosis, "relay_forwarding");
    let outcome = object_field(diagnosis, "outcome");

    let mut output = String::new();
    output.push_str(&format!(
        "Identity: {}\n",
        string_field(diagnosis, "identity", "unknown")
    ));
    output.push_str(&format!(
        "Provider key: {}\n",
        string_field(diagnosis, "provider_key", "unknown")
    ));
    output.push_str(&format!(
        "Local cache: {}",
        string_field(cache, "state", "unknown")
    ));
    if let Some(sequence) = cache.get("latest_sequence").and_then(Value::as_u64) {
        output.push_str(&format!(" (latest sequence {sequence})"));
    }
    output.push('\n');
    output.push_str(&format!(
        "Identity-head hint: {} ({} fresh / {} expired)\n",
        string_field(hint, "state", "unknown"),
        number_field(hint, "fresh_count"),
        number_field(hint, "expired_count")
    ));
    output.push_str(&format!(
        "Forwarding: {} ({} target{})\n",
        if bool_field(forwarding, "attempted") {
            "available"
        } else {
            "not available"
        },
        number_field(forwarding, "target_count"),
        if number_field(forwarding, "target_count") == 1 {
            ""
        } else {
            "s"
        }
    ));
    output.push_str(&format!(
        "Outcome: {}\n",
        string_field(outcome, "code", "unknown")
    ));
    if let Some(message) = outcome.get("message").and_then(Value::as_str) {
        output.push_str(&format!("{message}\n"));
    }

    if let Some(candidates) = diagnosis
        .get("provider_candidates")
        .and_then(Value::as_array)
        .filter(|candidates| !candidates.is_empty())
    {
        output.push_str("Candidates:\n");
        for candidate in candidates {
            let peer_id = string_field(candidate, "peer_id", "unknown");
            output.push_str(&format!("  {peer_id}\n"));
            if let Some(addrs) = candidate.get("addrs").and_then(Value::as_array) {
                for addr in addrs {
                    if let Some(addr) = addr.as_str() {
                        output.push_str(&format!("    {addr}\n"));
                    }
                }
            }
        }
    }

    output
}

fn object_field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or(&Value::Null)
}

fn string_field<'a>(value: &'a Value, key: &str, fallback: &'a str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or(fallback)
}

fn bool_field(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn number_field(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_relay_operator_status() {
        let status = serde_json::json!({
            "relay_enabled": true,
            "peer_id": "12D3Relay",
            "identity_address": "relayidentity.jolt",
            "listen_addresses": ["/ip4/0.0.0.0/tcp/4001"],
            "bootstrap": {
                "state": "connected",
                "configured_relay_count": 1,
                "effective_relay_count": 2,
                "connected_peer_count": 1,
                "last_error": null
            },
            "peers": {
                "connected": 8,
                "direct": 7,
                "relayed": 1
            },
            "known_relay_count": 3,
            "relay_record": { "body": { "peer_id": "12D3Relay" } },
            "cache": {
                "cached_items": 5,
                "cached_bytes": 2048,
                "published_items": 1,
                "published_bytes": 512,
                "pinned_items": 2,
                "pinned_bytes": 4096
            },
            "home_relay": null
        });

        let output = format_relay_status(&status, Some(9862));

        assert!(output.contains("Relay: enabled"));
        assert!(output.contains("API port: 9862"));
        assert!(output.contains("Bootstrap: connected (1 connected / 2 effective / 1 configured)"));
        assert!(output.contains("Known relays: 3"));
        assert!(output.contains("Peers: 8 connected (7 direct / 1 relayed)"));
        assert!(output.contains("Pins: 2 items / 4.0 KB"));
        assert!(output.contains("Relay record: present"));
        assert!(output.contains("/ip4/0.0.0.0/tcp/4001"));
    }

    #[test]
    fn formats_disabled_relay_status() {
        let status = serde_json::json!({
            "relay_enabled": false,
            "bootstrap": {},
            "peers": {},
            "cache": {},
            "relay_record": null,
            "home_relay": null
        });

        let output = format_relay_status(&status, None);

        assert!(output.contains("Relay: disabled"));
        assert!(output.contains("Relay record: none"));
        assert!(output.contains("Home relay: not configured"));
    }

    #[test]
    fn formats_identity_diagnosis() {
        let diagnosis = serde_json::json!({
            "identity": "abc123",
            "provider_key": "jolt:update-log:abc123",
            "local_update_log_cache": {
                "state": "miss",
                "entry_count": 0
            },
            "identity_head_hint": {
                "state": "miss",
                "fresh_count": 0,
                "expired_count": 0
            },
            "local_provider_candidates": [],
            "provider_candidates": [],
            "relay_forwarding": {
                "attempted": false,
                "target_count": 0,
                "attempts": []
            },
            "outcome": {
                "code": "no_bootstrap_relays",
                "message": "No update-log provider found for jolt:update-log:abc123"
            }
        });

        let output = format_identity_diagnosis(&diagnosis);

        assert!(output.contains("Identity: abc123"));
        assert!(output.contains("Provider key: jolt:update-log:abc123"));
        assert!(output.contains("Local cache: miss"));
        assert!(output.contains("Identity-head hint: miss (0 fresh / 0 expired)"));
        assert!(output.contains("Forwarding: not available (0 targets)"));
        assert!(output.contains("Outcome: no_bootstrap_relays"));
    }
}
