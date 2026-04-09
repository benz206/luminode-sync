#!/bin/sh
set -e

# Write a minimal streamrip config using the DEEZER_ARL environment variable.
# streamrip reads from ~/.config/streamrip/config.toml by default.
CONFIG_DIR="${HOME}/.config/streamrip"
CONFIG_FILE="${CONFIG_DIR}/config.toml"

mkdir -p "${CONFIG_DIR}"

if [ -z "${DEEZER_ARL}" ]; then
    echo "WARNING: DEEZER_ARL is not set — streamrip will not be able to authenticate with Deezer."
    echo "         Set DEEZER_ARL to your Deezer ARL cookie value (log in to deezer.com, open"
    echo "         DevTools → Application → Cookies → copy the 'arl' cookie)."
fi

cat > "${CONFIG_FILE}" << EOF
[deezer]
arl = "${DEEZER_ARL}"
quality = 2

[downloads]
folder = "/tmp/streamrip"
disc_subdirectories = false
concurrency = false

[conversion]
enabled = false

[filepaths]
add_singles_to_folder = false
folder_format = "{albumartist}/{album}"
track_format = "{tracknumber}. {title}"
EOF

echo "streamrip config written to ${CONFIG_FILE}"

exec "$@"
