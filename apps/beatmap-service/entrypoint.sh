#!/bin/sh
set -e

if [ -z "${DEEZER_ARL}" ]; then
    echo "WARNING: DEEZER_ARL is not set — streamrip will not authenticate with Deezer."
    echo "         Set DEEZER_ARL to your Deezer ARL cookie value:"
    echo "         Log in to deezer.com → DevTools → Application → Cookies → 'arl'"
fi

# Generate a complete default config (avoids missing-key errors from hand-written TOML).
echo "Initialising streamrip config..."
rip config reset

# Patch ARL and download folder into the generated config.
python3 - <<'PYEOF'
import os, re

config_file = os.path.expanduser("~/.config/streamrip/config.toml")
arl         = os.environ.get("DEEZER_ARL", "")
work_dir    = "/tmp/streamrip-work"

with open(config_file) as f:
    content = f.read()

def patch(pattern, replacement, text):
    """Safe substitution — replacement is treated as a literal string."""
    return re.sub(pattern, lambda m: m.group(1) + replacement, text, flags=re.MULTILINE)

content = patch(r"^(arl\s*=\s*).*$",    f'"{arl}"',     content)
content = patch(r"^(folder\s*=\s*).*$", f'"{work_dir}"', content)

with open(config_file, "w") as f:
    f.write(content)

print(f"streamrip config patched: arl={len(arl)} chars, folder={work_dir}")
PYEOF

exec "$@"
