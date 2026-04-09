#!/bin/sh
set -e

if [ -z "${DEEZER_ARL}" ]; then
    echo "INFO: DEEZER_ARL not set — downloads will fall back to yt-dlp (YouTube)."
    echo "      For higher quality, create a free account at deezer.com, log in,"
    echo "      then open DevTools → Application → Cookies → copy the 'arl' value."
fi

# Create the streamrip config via the Python API — avoids 'rip config reset'
# which opens /dev/tty for its confirmation prompt and cannot be piped.
python3 - <<'PYEOF'
import os, sys, pathlib

config_path = pathlib.Path.home() / ".config/streamrip/config.toml"
config_path.parent.mkdir(parents=True, exist_ok=True)

# Delete any existing (possibly corrupt/incomplete) config so streamrip
# regenerates full defaults when Config() is first instantiated.
config_path.unlink(missing_ok=True)

arl      = os.environ.get("DEEZER_ARL", "")
work_dir = "/tmp/streamrip-work"

try:
    from streamrip.config import Config
    cfg = Config(str(config_path))       # creates + saves defaults if file absent
    cfg.session.deezer.arl              = arl
    cfg.session.downloads.folder        = work_dir
    cfg.session.misc.check_for_updates  = False
    cfg.save()
    print(f"streamrip config written to {config_path} (ARL: {len(arl)} chars, folder: {work_dir})")
except Exception as e:
    print(f"ERROR: could not write streamrip config: {e}", file=sys.stderr)
    sys.exit(1)
PYEOF

exec "$@"
