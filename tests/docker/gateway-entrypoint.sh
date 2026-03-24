#!/bin/bash
set -e

# IP forwarding is set via sysctls in docker-compose.yml
# Just set up iptables NAT rules

iptables -t nat -A POSTROUTING -j MASQUERADE
iptables -A FORWARD -j ACCEPT

echo "NAT gateway ready"

# Keep container running
exec sleep infinity
