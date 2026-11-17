#!/bin/sh
# Tailscale for ANGI, supervised by agx-init (installed by scripts/deploy-tailscale.sh).
#
# Userspace networking only: the kernel has no TUN device, and nothing here changes routes, DNS
# or firewall rules, so SSH and the Pi's own networking are untouched. The node is logged in with
# shields up, so nothing on the tailnet can connect in (the desktop VNC listens on loopback
# without a password and must not become reachable). The only use is outbound: a loopback relay
# (see inetd.conf) carries Agent Angie's LM Studio traffic to its tailnet address.
base=/var/lib/agx-data/tailscale
socket=/run/tailscale/tailscaled.sock
relay_pid=/run/tailscale/relay.pid

# The data volume is mounted by an earlier boot step; wait briefly rather than fail the supervisor.
n=0
while ! mountpoint -q /var/lib/agx-data; do
    n=$((n + 1)); [ "$n" -gt 60 ] && { echo "AGX data volume is not mounted" >&2; exit 1; }
    sleep 1
done
umask 077
mkdir -p "$base/state" /run/tailscale
# The settings panel (running as the desktop user) writes the LM Studio address here.
mkdir -p "$base/config" && chown 0:agx-admin "$base/config" && chmod 2770 "$base/config"

[ -r "$relay_pid" ] && kill "$(cat "$relay_pid")" 2>/dev/null
inetd -f /usr/libexec/agx-tailscale/inetd.conf &
relay=$!
echo "$relay" > "$relay_pid"
/usr/sbin/tailscaled --tun=userspace-networking --state="$base/state/tailscaled.state" --socket="$socket" &
daemon=$!
# Re-assert the safe preferences once the daemon answers, in case any were reset by a sign-in.
(
    n=0
    until /usr/bin/tailscale --socket="$socket" set --shields-up=true --accept-dns=false --accept-routes=false 2>/dev/null; do
        n=$((n + 1)); [ "$n" -gt 30 ] && break
        sleep 1
    done
) &
trap 'kill "$daemon" "$relay" 2>/dev/null' TERM INT
wait "$daemon"
status=$?
kill "$relay" 2>/dev/null
exit "$status"
