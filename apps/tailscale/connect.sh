#!/bin/sh
# One relayed connection from the loopback relay (inetd.conf) to LM Studio over Tailscale. The
# target is whatever address the settings panel last tested; it is checked because the panel's
# user can write it and this runs as root.
read -r host port < /var/lib/agx-data/tailscale/config/lmstudio-target || exit 1
case "$host" in ''|-*|*[!A-Za-z0-9._-]*) exit 1 ;; esac
case "$port" in ''|*[!0-9]*) exit 1 ;; esac
exec /usr/bin/tailscale --socket=/run/tailscale/tailscaled.sock nc "$host" "$port"
