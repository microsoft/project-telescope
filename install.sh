#!/usr/bin/env bash
# Project Telescope installer for macOS and Linux
# Usage: curl -fsSL https://raw.githubusercontent.com/microsoft/project-telescope/main/install.sh | bash

set -euo pipefail

REPO="microsoft/project-telescope"
INSTALL_DIR="${TELESCOPE_INSTALL_DIR:-$HOME/.telescope}"
BIN_DIR="$INSTALL_DIR/bin"
VERSION="${TELESCOPE_VERSION:-latest}"

info() { printf '\033[1;34m%s\033[0m\n' "$*"; }
error() { printf '\033[1;31mError: %s\033[0m\n' "$*" >&2; exit 1; }

detect_platform() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux*)  OS="linux" ;;
        Darwin*) OS="macos" ;;
        *)       error "Unsupported operating system: $os" ;;
    esac

    case "$arch" in
        x86_64|amd64)  ARCH="x64" ;;
        arm64|aarch64) ARCH="arm64" ;;
        *)             error "Unsupported architecture: $arch" ;;
    esac
}

get_download_url() {
    local asset_name="telescope-${OS}-${ARCH}.zip"

    if [ "$VERSION" = "latest" ]; then
        DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/${asset_name}"
    else
        DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${asset_name}"
    fi
}

install() {
    detect_platform
    get_download_url

    info "Installing Project Telescope for ${OS}/${ARCH}..."
    info "Download: ${DOWNLOAD_URL}"

    local tmp_dir
    tmp_dir="$(mktemp -d)"
    trap 'rm -rf "$tmp_dir"' EXIT

    # Download
    if command -v curl &>/dev/null; then
        curl -fsSL "$DOWNLOAD_URL" -o "$tmp_dir/telescope.zip"
    elif command -v wget &>/dev/null; then
        wget -q "$DOWNLOAD_URL" -O "$tmp_dir/telescope.zip"
    else
        error "curl or wget is required to download Project Telescope"
    fi

    # Extract
    if ! command -v unzip &>/dev/null; then
        error "unzip is required to install Project Telescope"
    fi
    unzip -qo "$tmp_dir/telescope.zip" -d "$tmp_dir/extracted"

    # Install binaries
    mkdir -p "$BIN_DIR"
    find "$tmp_dir/extracted" -type f ! -name '*.d' -exec cp {} "$BIN_DIR/" \;
    chmod +x "$BIN_DIR"/*

    info "Installed to ${BIN_DIR}"

    # Add to PATH
    add_to_path
}

add_to_path() {
    local shell_config=""
    local path_line="export PATH=\"${BIN_DIR}:\$PATH\""

    if [ -n "${BASH_VERSION:-}" ] || [ "$(basename "$SHELL")" = "bash" ]; then
        if [ -f "$HOME/.bashrc" ]; then
            shell_config="$HOME/.bashrc"
        elif [ -f "$HOME/.bash_profile" ]; then
            shell_config="$HOME/.bash_profile"
        fi
    fi

    if [ "$(basename "${SHELL:-}")" = "zsh" ] || [ -f "$HOME/.zshrc" ]; then
        shell_config="$HOME/.zshrc"
    fi

    if [ -n "$shell_config" ]; then
        if ! grep -q '.telescope/bin' "$shell_config" 2>/dev/null; then
            echo "" >> "$shell_config"
            echo "# Project Telescope" >> "$shell_config"
            echo "$path_line" >> "$shell_config"
            info "Added ${BIN_DIR} to PATH in ${shell_config}"
        fi
    fi

    # Check if already on PATH
    if echo "$PATH" | tr ':' '\n' | grep -q "$BIN_DIR"; then
        info "Project Telescope is ready! Run 'tele --help' to get started."
    else
        info "Project Telescope is installed! Restart your shell or run:"
        info "  export PATH=\"${BIN_DIR}:\$PATH\""
        info "Then run 'tele --help' to get started."
    fi
}

install
