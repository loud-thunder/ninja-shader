#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${REPO_DIR}"

echo "==> Building ninja-shader in release mode..."
cargo build --release

TARGET_BIN="${REPO_DIR}/target/release/ninja-shader"

if [[ ! -f "${TARGET_BIN}" ]]; then
    echo "Error: Binary not found at ${TARGET_BIN}" >&2
    exit 1
fi

DEST_DIR="/usr/local/bin"
DEST_BIN="${DEST_DIR}/ninja-shader"
USE_SUDO=false

if [[ -w "${DEST_DIR}" ]]; then
    echo "==> ${DEST_DIR} is writable. Creating symlink..."
else
    echo "==> ${DEST_DIR} requires elevated privileges."
    if [[ -t 0 ]]; then
        echo "Choose installation destination:"
        echo "  [1] Elevate with sudo -> ${DEST_BIN}"
        echo "  [2] Install to user directory -> ${HOME}/.local/bin/ninja-shader"
        read -r -p "Select option [1/2] (default: 1): " CHOICE
        case "${CHOICE}" in
            2)
                DEST_DIR="${HOME}/.local/bin"
                DEST_BIN="${DEST_DIR}/ninja-shader"
                mkdir -p "${DEST_DIR}"
                ;;
            *)
                USE_SUDO=true
                ;;
        esac
    else
        echo "==> Non-interactive session detected. Falling back to ${HOME}/.local/bin"
        DEST_DIR="${HOME}/.local/bin"
        DEST_BIN="${DEST_DIR}/ninja-shader"
        mkdir -p "${DEST_DIR}"
    fi
fi

if [[ "${USE_SUDO}" == "true" ]]; then
    echo "==> Symlinking via sudo: ${DEST_BIN} -> ${TARGET_BIN}"
    sudo ln -sf "${TARGET_BIN}" "${DEST_BIN}"
else
    echo "==> Symlinking: ${DEST_BIN} -> ${TARGET_BIN}"
    ln -sf "${TARGET_BIN}" "${DEST_BIN}"
fi

echo "==> Successfully linked ninja-shader to ${DEST_BIN}"

# Check PATH
case ":${PATH}:" in
    *:"${DEST_DIR}":*) ;;
    *)
        echo "Warning: ${DEST_DIR} is not in your PATH. You may need to add it: export PATH=\"${DEST_DIR}:\$PATH\""
        ;;
esac
