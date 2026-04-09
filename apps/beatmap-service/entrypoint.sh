#!/bin/sh
set -e

# Generate a complete default streamrip config (avoids missing-key errors from
# handwritten TOML — streamrip validates all sections at startup).
echo "Initialising streamrip config…"
yes | rip config reset 2>/dev/null || rip config reset || true

CONFIG_FILE="${HOME}/.config/streamrip/config.toml"

# Patch the Deezer ARL into the reset config using Python (no extra deps needed).
python3 - <<'PYEOF'
import os, re

config_file = os.path.expanduser("~/.config/streamrip/config.toml")
arl = os.environ.get("DEEZER_ARL", "")

if not arl:
    print("WARNING: DEEZER_ARL is not set — streamrip will not authenticate with Deezer.")
    print("         Set DEEZER_ARL to your Deezer ARL cookie value:")
    print("         Log in to deezer.com → DevTools → Application → Cookies → 'arl'")

with open(config_file) as f:
    content = f.read()

# Replace the arl line in the [deezer] section.
content = re.sub(r'(?m)^(arl\s*=\s*).*$', f'\\g<1>"{arl}"', content)

with open(config_file, "w") as f:
    f.write(content)

print(f"streamrip config written: {config_file} (ARL length: {len(arl)})")
PYEOF

exec "$@"
