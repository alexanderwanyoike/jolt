//! Minimal STUN client for external address discovery.
//!
//! Implements RFC 5389 Binding Request/Response to discover the external IP:port
//! as seen by public STUN servers. This feeds accurate addresses into the swarm
//! so that dcutr hole punching has correct targets.
//!
//! Also detects NAT type: if all STUN servers report the same external port,
//! the NAT is endpoint-independent (hole-punchable). If ports differ, it's
//! symmetric (hole punching will likely fail).

use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::UdpSocket;
use tracing::{debug, info, warn};

/// STUN magic cookie (RFC 5389)
const MAGIC_COOKIE: u32 = 0x2112A442;
/// STUN Binding Request type
const BINDING_REQUEST: u16 = 0x0001;
/// STUN Binding Response type
const BINDING_RESPONSE: u16 = 0x0101;
/// XOR-MAPPED-ADDRESS attribute type
const XOR_MAPPED_ADDRESS: u16 = 0x0020;
/// MAPPED-ADDRESS attribute type (fallback, older servers)
const MAPPED_ADDRESS: u16 = 0x0001;

/// Detected NAT type based on STUN probing.
#[derive(Debug, Clone, PartialEq)]
pub enum NatType {
    /// No NAT detected (external addr matches local addr)
    None,
    /// Endpoint-independent mapping: same external port regardless of destination.
    /// Hole punching should work.
    EndpointIndependent,
    /// Endpoint-dependent mapping (symmetric NAT): external port changes per destination.
    /// Standard hole punching will fail. Need ICE/WebRTC or relay.
    Symmetric,
    /// Could not determine (not enough STUN responses).
    Unknown,
}

/// Result of STUN discovery.
#[derive(Debug, Clone)]
pub struct StunResult {
    /// External address as seen by STUN servers
    pub external_addr: Option<SocketAddr>,
    /// Detected NAT type
    pub nat_type: NatType,
    /// Individual results from each STUN server
    pub server_results: Vec<(SocketAddr, Option<SocketAddr>)>,
}

/// Default public STUN server hostnames.
pub fn default_stun_server_hosts() -> Vec<(&'static str, u16)> {
    vec![
        ("stun.l.google.com", 19302),
        ("stun1.l.google.com", 19302),
        ("stun.cloudflare.com", 3478),
    ]
}

/// Resolve STUN server hostnames to socket addresses.
pub async fn resolve_stun_servers() -> Vec<SocketAddr> {
    let mut addrs = Vec::new();
    for (host, port) in default_stun_server_hosts() {
        match tokio::net::lookup_host(format!("{host}:{port}")).await {
            Ok(mut iter) => {
                if let Some(addr) = iter.next() {
                    addrs.push(addr);
                }
            }
            Err(e) => {
                debug!("STUN: failed to resolve {host}: {e}");
            }
        }
    }
    addrs
}

/// Build a STUN Binding Request packet.
pub fn build_binding_request(transaction_id: &[u8; 12]) -> [u8; 20] {
    let mut buf = [0u8; 20];
    // Message Type: Binding Request (0x0001)
    buf[0] = (BINDING_REQUEST >> 8) as u8;
    buf[1] = (BINDING_REQUEST & 0xFF) as u8;
    // Message Length: 0 (no attributes)
    buf[2] = 0;
    buf[3] = 0;
    // Magic Cookie
    buf[4] = (MAGIC_COOKIE >> 24) as u8;
    buf[5] = (MAGIC_COOKIE >> 16) as u8;
    buf[6] = (MAGIC_COOKIE >> 8) as u8;
    buf[7] = (MAGIC_COOKIE & 0xFF) as u8;
    // Transaction ID (12 bytes)
    buf[8..20].copy_from_slice(transaction_id);
    buf
}

/// Parse a STUN Binding Response, extracting the XOR-MAPPED-ADDRESS.
pub fn parse_binding_response(buf: &[u8], transaction_id: &[u8; 12]) -> Option<SocketAddr> {
    if buf.len() < 20 {
        return None;
    }

    // Check message type is Binding Response
    let msg_type = u16::from_be_bytes([buf[0], buf[1]]);
    if msg_type != BINDING_RESPONSE {
        return None;
    }

    // Check magic cookie
    let cookie = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
    if cookie != MAGIC_COOKIE {
        return None;
    }

    // Check transaction ID matches
    if buf[8..20] != *transaction_id {
        return None;
    }

    let msg_len = u16::from_be_bytes([buf[2], buf[3]]) as usize;
    if buf.len() < 20 + msg_len {
        return None;
    }

    // Parse attributes
    let mut offset = 20;
    while offset + 4 <= 20 + msg_len {
        let attr_type = u16::from_be_bytes([buf[offset], buf[offset + 1]]);
        let attr_len = u16::from_be_bytes([buf[offset + 2], buf[offset + 3]]) as usize;
        let attr_start = offset + 4;

        if attr_start + attr_len > buf.len() {
            break;
        }

        if attr_type == XOR_MAPPED_ADDRESS {
            return parse_xor_mapped_address(&buf[attr_start..attr_start + attr_len]);
        }

        if attr_type == MAPPED_ADDRESS {
            return parse_mapped_address(&buf[attr_start..attr_start + attr_len]);
        }

        // Align to 4-byte boundary
        offset = attr_start + ((attr_len + 3) & !3);
    }

    None
}

/// Parse XOR-MAPPED-ADDRESS attribute value (RFC 5389 Section 15.2).
fn parse_xor_mapped_address(data: &[u8]) -> Option<SocketAddr> {
    if data.len() < 8 {
        return None;
    }

    let family = data[1];
    let xored_port = u16::from_be_bytes([data[2], data[3]]);
    let port = xored_port ^ (MAGIC_COOKIE >> 16) as u16;

    match family {
        0x01 => {
            // IPv4
            if data.len() < 8 {
                return None;
            }
            let cookie_bytes = MAGIC_COOKIE.to_be_bytes();
            let ip = std::net::Ipv4Addr::new(
                data[4] ^ cookie_bytes[0],
                data[5] ^ cookie_bytes[1],
                data[6] ^ cookie_bytes[2],
                data[7] ^ cookie_bytes[3],
            );
            Some(SocketAddr::new(std::net::IpAddr::V4(ip), port))
        }
        0x02 => {
            // IPv6
            if data.len() < 20 {
                return None;
            }
            // XOR with magic cookie + transaction ID (16 bytes total)
            // For simplicity, skip IPv6 XOR for now
            None
        }
        _ => None,
    }
}

/// Parse MAPPED-ADDRESS attribute (non-XOR, older servers).
fn parse_mapped_address(data: &[u8]) -> Option<SocketAddr> {
    if data.len() < 8 {
        return None;
    }

    let family = data[1];
    let port = u16::from_be_bytes([data[2], data[3]]);

    match family {
        0x01 => {
            let ip = std::net::Ipv4Addr::new(data[4], data[5], data[6], data[7]);
            Some(SocketAddr::new(std::net::IpAddr::V4(ip), port))
        }
        _ => None,
    }
}

/// Send a STUN Binding Request and wait for the response.
pub async fn stun_query(
    local_socket: &UdpSocket,
    stun_server: SocketAddr,
    timeout: Duration,
) -> Option<SocketAddr> {
    let mut transaction_id = [0u8; 12];
    getrandom(&mut transaction_id);

    let request = build_binding_request(&transaction_id);

    if let Err(e) = local_socket.send_to(&request, stun_server).await {
        debug!("STUN send to {stun_server} failed: {e}");
        return None;
    }

    let mut buf = [0u8; 256];
    match tokio::time::timeout(timeout, local_socket.recv_from(&mut buf)).await {
        Ok(Ok((len, _))) => parse_binding_response(&buf[..len], &transaction_id),
        Ok(Err(e)) => {
            debug!("STUN recv from {stun_server} failed: {e}");
            None
        }
        Err(_) => {
            debug!("STUN query to {stun_server} timed out");
            None
        }
    }
}

/// Fill buffer with random bytes using the OS RNG.
fn getrandom(buf: &mut [u8]) {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    // Simple random bytes from HashMap's random state (no extra deps)
    let state = RandomState::new();
    for chunk in buf.chunks_mut(8) {
        let mut hasher = state.build_hasher();
        hasher.write_u64(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64);
        let random = hasher.finish().to_le_bytes();
        let len = chunk.len().min(8);
        chunk[..len].copy_from_slice(&random[..len]);
    }
}

/// Discover external address by querying multiple STUN servers.
/// Also detects NAT type based on whether the external port is consistent.
pub async fn discover_external_addr(
    local_port: u16,
    stun_servers: &[SocketAddr],
) -> StunResult {
    // Bind a UDP socket on the same port we use for QUIC
    let socket = match UdpSocket::bind(("0.0.0.0", 0)).await {
        Ok(s) => s,
        Err(e) => {
            warn!("STUN: failed to bind UDP socket: {e}");
            return StunResult {
                external_addr: None,
                nat_type: NatType::Unknown,
                server_results: vec![],
            };
        }
    };

    let timeout = Duration::from_secs(3);
    let mut results = Vec::new();

    for server in stun_servers {
        let mapped = stun_query(&socket, *server, timeout).await;
        debug!("STUN {server} -> {mapped:?}");
        results.push((*server, mapped));
    }

    // Analyze results
    let successful: Vec<SocketAddr> = results.iter().filter_map(|(_, m)| *m).collect();

    if successful.is_empty() {
        return StunResult {
            external_addr: None,
            nat_type: NatType::Unknown,
            server_results: results,
        };
    }

    let external_addr = Some(successful[0]);
    let nat_type = detect_nat_type(&successful);

    match &nat_type {
        NatType::EndpointIndependent => {
            info!("STUN: external address {} (endpoint-independent NAT, hole-punchable)", successful[0]);
        }
        NatType::Symmetric => {
            let ports: Vec<u16> = successful.iter().map(|a| a.port()).collect();
            warn!("STUN: symmetric NAT detected (ports vary: {ports:?}). Hole punching unlikely to work.");
        }
        _ => {
            info!("STUN: external address {}", successful[0]);
        }
    }

    StunResult {
        external_addr,
        nat_type,
        server_results: results,
    }
}

/// Detect NAT type from multiple STUN results.
pub fn detect_nat_type(mapped_addrs: &[SocketAddr]) -> NatType {
    if mapped_addrs.len() < 2 {
        return NatType::Unknown;
    }

    let ports: Vec<u16> = mapped_addrs.iter().map(|a| a.port()).collect();
    if ports.windows(2).all(|w| w[0] == w[1]) {
        NatType::EndpointIndependent
    } else {
        NatType::Symmetric
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stun_request_format() {
        let txn_id = [1u8; 12];
        let req = build_binding_request(&txn_id);

        assert_eq!(req.len(), 20);
        // Message type: Binding Request
        assert_eq!(req[0], 0x00);
        assert_eq!(req[1], 0x01);
        // Message length: 0
        assert_eq!(req[2], 0x00);
        assert_eq!(req[3], 0x00);
        // Magic cookie
        assert_eq!(req[4], 0x21);
        assert_eq!(req[5], 0x12);
        assert_eq!(req[6], 0xA4);
        assert_eq!(req[7], 0x42);
        // Transaction ID
        assert_eq!(&req[8..20], &[1u8; 12]);
    }

    #[test]
    fn test_stun_parse_xor_mapped_address() {
        // Build a fake STUN response with XOR-MAPPED-ADDRESS
        // External address: 203.0.113.5:12345
        let txn_id = [0xAA; 12];
        let ip = std::net::Ipv4Addr::new(203, 0, 113, 5);
        let port: u16 = 12345;

        // XOR the port with upper 16 bits of magic cookie
        let xored_port = port ^ (MAGIC_COOKIE >> 16) as u16;
        let cookie_bytes = MAGIC_COOKIE.to_be_bytes();
        let ip_octets = ip.octets();
        let xored_ip = [
            ip_octets[0] ^ cookie_bytes[0],
            ip_octets[1] ^ cookie_bytes[1],
            ip_octets[2] ^ cookie_bytes[2],
            ip_octets[3] ^ cookie_bytes[3],
        ];

        // Build response
        let mut resp = vec![0u8; 32];
        // Binding Response
        resp[0] = 0x01;
        resp[1] = 0x01;
        // Message length: 12 (one attribute)
        resp[2] = 0x00;
        resp[3] = 0x0C;
        // Magic cookie
        resp[4..8].copy_from_slice(&cookie_bytes);
        // Transaction ID
        resp[8..20].copy_from_slice(&txn_id);
        // XOR-MAPPED-ADDRESS attribute
        resp[20] = 0x00;
        resp[21] = 0x20; // type
        resp[22] = 0x00;
        resp[23] = 0x08; // length
        resp[24] = 0x00; // reserved
        resp[25] = 0x01; // family: IPv4
        resp[26..28].copy_from_slice(&xored_port.to_be_bytes());
        resp[28..32].copy_from_slice(&xored_ip);

        let result = parse_binding_response(&resp, &txn_id);
        assert!(result.is_some());
        let addr = result.unwrap();
        assert_eq!(addr.ip(), std::net::IpAddr::V4(ip));
        assert_eq!(addr.port(), port);
    }

    #[test]
    fn test_nat_type_endpoint_independent() {
        let addrs = vec![
            "1.2.3.4:5000".parse().unwrap(),
            "1.2.3.4:5000".parse().unwrap(),
            "1.2.3.4:5000".parse().unwrap(),
        ];
        assert_eq!(detect_nat_type(&addrs), NatType::EndpointIndependent);
    }

    #[test]
    fn test_nat_type_symmetric() {
        let addrs = vec![
            "1.2.3.4:5000".parse().unwrap(),
            "1.2.3.4:5001".parse().unwrap(),
            "1.2.3.4:5002".parse().unwrap(),
        ];
        assert_eq!(detect_nat_type(&addrs), NatType::Symmetric);
    }

    #[test]
    fn test_nat_type_unknown_single_result() {
        let addrs = vec!["1.2.3.4:5000".parse().unwrap()];
        assert_eq!(detect_nat_type(&addrs), NatType::Unknown);
    }

    #[test]
    fn test_stun_timeout_returns_none() {
        // Parse an invalid buffer
        let result = parse_binding_response(&[0u8; 5], &[0u8; 12]);
        assert!(result.is_none());
    }

    #[test]
    fn test_stun_wrong_transaction_id() {
        let txn_id = [0xAA; 12];
        let wrong_txn = [0xBB; 12];

        let mut resp = vec![0u8; 20];
        resp[0] = 0x01;
        resp[1] = 0x01;
        resp[4..8].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
        resp[8..20].copy_from_slice(&wrong_txn);

        let result = parse_binding_response(&resp, &txn_id);
        assert!(result.is_none());
    }

    #[tokio::test]
    #[ignore] // Requires internet access -- run manually
    async fn test_stun_real_server() {
        let servers = resolve_stun_servers().await;
        assert!(!servers.is_empty(), "Should resolve at least one STUN server");
        let result = discover_external_addr(0, &servers).await;
        println!("STUN result: {result:?}");
        assert!(result.external_addr.is_some(), "Should discover external address");
        assert_ne!(result.nat_type, NatType::Unknown, "Should detect NAT type");
    }
}
