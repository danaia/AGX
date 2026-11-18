#!/bin/bash
# ANGI's guarded desktop updater. Runs as the desktop account; root steps use angi-root.
set -euo pipefail
mountpoint -q /var/lib/agx-data || { echo 'angi-update needs the NVMe data volume.' >&2; exit 1; }
exec /usr/bin/python3 /usr/libexec/agx-agent/update.py "$@"
