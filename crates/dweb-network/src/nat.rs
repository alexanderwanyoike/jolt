use std::net::IpAddr;
use std::num::NonZeroU16;

use tracing::{debug, info, warn};

/// Attempt NAT-PMP/PCP port mapping for the given local port.
/// Tries PCP first, falls back to NAT-PMP.
/// Returns the external address if successful.
pub async fn try_port_mapping(
    local_port: u16,
    protocol: crab_nat::InternetProtocol,
) -> Option<(IpAddr, u16)> {
    let local_address = match netdev::interface::get_local_ipaddr() {
        Some(addr) => addr,
        None => {
            debug!("NAT-PMP: could not determine local address");
            return None;
        }
    };

    let gateway = match detect_gateway() {
        Some(gw) => gw,
        None => {
            debug!("NAT-PMP: could not determine gateway");
            return None;
        }
    };

    info!("NAT-PMP: attempting port mapping local={local_port} via gateway={gateway}");

    let internal_port = match NonZeroU16::new(local_port) {
        Some(p) => p,
        None => {
            debug!("NAT-PMP: invalid port 0");
            return None;
        }
    };

    match crab_nat::PortMapping::new(
        gateway,
        local_address,
        protocol,
        internal_port,
        crab_nat::PortMappingOptions::default(),
    )
    .await
    {
        Ok(mapping) => {
            let external_port = mapping.external_port().get();
            info!(
                "NAT-PMP: mapped external port {external_port} -> local port {local_port} (lifetime: {}s)",
                mapping.lifetime()
            );
            // Return the gateway's public IP with the mapped port
            // Note: on CGNAT this will be the CGNAT IP, not useful
            Some((gateway, external_port))
        }
        Err(e) => {
            debug!("NAT-PMP: port mapping failed: {e}");
            None
        }
    }
}

/// Attempt TCP and UDP port mappings simultaneously.
pub async fn try_all_mappings(tcp_port: u16, udp_port: u16) {
    let tcp = tokio::spawn(async move {
        try_port_mapping(tcp_port, crab_nat::InternetProtocol::Tcp).await
    });

    let udp = tokio::spawn(async move {
        try_port_mapping(udp_port, crab_nat::InternetProtocol::Udp).await
    });

    if let Ok(Some((ip, port))) = tcp.await {
        info!("NAT-PMP: TCP mapped to {ip}:{port}");
    }

    if let Ok(Some((ip, port))) = udp.await {
        info!("NAT-PMP: UDP mapped to {ip}:{port}");
    }
}

fn detect_gateway() -> Option<IpAddr> {
    match netdev::get_default_gateway() {
        Ok(gw) => {
            if let Some(ipv4) = gw.ipv4.first() {
                Some(IpAddr::V4(*ipv4))
            } else if let Some(ipv6) = gw.ipv6.first() {
                Some(IpAddr::V6(*ipv6))
            } else {
                warn!("NAT-PMP: gateway found but no IP addresses");
                None
            }
        }
        Err(e) => {
            debug!("NAT-PMP: gateway detection failed: {e}");
            None
        }
    }
}
