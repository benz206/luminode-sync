#!/bin/sh
set -e

if [ -z "${DEEZER_ARL}" ]; then
    echo "INFO: DEEZER_ARL not set — downloads will use yt-dlp (YouTube) fallback."
    echo "      For higher quality, create a free account at deezer.com, log in,"
    echo "      then open DevTools → Application → Cookies → copy the 'arl' value."
fi

CONFIG_FILE="${HOME}/.config/streamrip/config.toml"

# Remove any existing (possibly corrupt/incomplete) config so streamrip
# regenerates a full default on next startup.
rm -f "${CONFIG_FILE}"

# Run a non-destructive rip subcommand to trigger the Click group callback,
# which auto-creates the default config when the file is absent.
# 'rip config path' just prints the config path — no prompts, no side effects.
echo "Generating streamrip default config..."
rip config path

# Patch ARL and download folder into the generated config using Python.
# The lambda replacement avoids treating the replacement string as a regex.
python3 - <<'PYEOF'
import os, re, pathlib, sys

config_file = pathlib.Path.home() / ".config/streamrip/config.toml"
arl      = os.environ.get("DEEZER_ARL", "")
work_dir = "/tmp/streamrip-work"

try:
    content = config_file.read_text()
except FileNotFoundError:
    print(f"ERROR: streamrip did not create {config_file}", file=sys.stderr)
    sys.exit(1)

def patch(pattern, value, text):
    """Replace a TOML key's value; treats value as a literal string."""
    return re.sub(pattern, lambda m: m.group(1) + value, text, flags=re.MULTILINE)

content = patch(r"^(arl\s*=\s*).*$",              f'"{arl}"',     content)
content = patch(r"^(folder\s*=\s*).*$",            f'"{work_dir}"', content)
content = patch(r"^(check_for_updates\s*=\s*).*$", "false",        content)

config_file.write_text(content)
print(f"Config patched → arl={len(arl)} chars, folder={work_dir}")
PYEOF

exec "$@"
