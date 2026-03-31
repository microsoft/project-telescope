#!/usr/bin/env bash
# Project Telescope Release Installer for Linux, macOS, and WSL
# Usage: bash install.sh
#
# Installs Telescope from an extracted release archive to ~/.telescope/bin/,
# and adds to PATH. This script is bundled into release archives.
# This script is idempotent — safe to run multiple times.

# CRLF self-heal: if this file has Windows line endings, strip them and re-exec.
{ head -1 "$0" | grep -q $'\r' ; } 2>/dev/null && exec bash <(tr -d '\r' < "$0") "$@" ; # CRLF-safe

set -euo pipefail

TELESCOPE_DIR="$HOME/.telescope"
BIN_DIR="$TELESCOPE_DIR/bin"
TELE_EXE="$BIN_DIR/tele"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TOTAL_STEPS=3
STEP=0

# ── Helpers ────────────────────────────────────────────

step()  { STEP=$((STEP + 1)); printf "  [%d/%d] %s\n" "$STEP" "$TOTAL_STEPS" "$1"; }
ok()    { printf "       \033[32m%s\033[0m\n" "$1"; }
skip()  { printf "       \033[90m%s\033[0m\n" "$1"; }
warn()  { printf "       \033[33m%s\033[0m\n" "$1"; }

# ── Banner ─────────────────────────────────────────────

echo ""
printf "  \033[36mTelescope Installer\033[0m\n"
printf "  \033[90m-------------------------------------------\033[0m\n"
echo ""

# ── Step 1: Install binaries ──────────────────────────

step "Installing binaries..."

mkdir -p "$BIN_DIR"

# Stop running instance so we can overwrite
if pgrep -x tele > /dev/null 2>&1 || pgrep -x telescope-service > /dev/null 2>&1; then
    pkill -x tele 2>/dev/null || true
    pkill -x telescope-service 2>/dev/null || true
    sleep 0.5
fi

# Copy binary files from the archive
for f in "$SCRIPT_DIR"/tele "$SCRIPT_DIR"/telescope-*; do
    [ -f "$f" ] && cp -f "$f" "$BIN_DIR/" 2>/dev/null || true
done

chmod +x "$BIN_DIR"/tele* 2>/dev/null || true

if [ -f "$TELE_EXE" ]; then
    SIZE=$(du -h "$TELE_EXE" | cut -f1)
    ok "Installed tele ($SIZE) to $BIN_DIR"
else
    warn "tele binary not found in archive"
fi

# ── Step 2: PATH ───────────────────────────────────────

step "Configuring PATH..."

add_to_path() {
    local shell_rc="$1"
    local export_line="export PATH=\"$BIN_DIR:\$PATH\""

    if [ -f "$shell_rc" ] && grep -qF "$BIN_DIR" "$shell_rc" 2>/dev/null; then
        return 1  # Already present
    fi

    echo "" >> "$shell_rc"
    echo "# Telescope" >> "$shell_rc"
    echo "$export_line" >> "$shell_rc"
    return 0
}

PATH_ADDED=false

CURRENT_SHELL="$(basename "${SHELL:-/bin/bash}")"
case "$CURRENT_SHELL" in
    zsh)
        if add_to_path "$HOME/.zshrc"; then
            ok "Added to ~/.zshrc"
            PATH_ADDED=true
        fi
        ;;
    bash)
        if add_to_path "$HOME/.bashrc"; then
            ok "Added to ~/.bashrc"
            PATH_ADDED=true
        fi
        ;;
    *)
        if add_to_path "$HOME/.profile"; then
            ok "Added to ~/.profile"
            PATH_ADDED=true
        fi
        ;;
esac

if [ "$PATH_ADDED" = false ]; then
    skip "Already on PATH"
fi

export PATH="$BIN_DIR:$PATH"

# ── Step 3: Verify installation ───────────────────────

step "Verifying installation..."

if [ -f "$TELE_EXE" ] && "$TELE_EXE" --version > /dev/null 2>&1; then
    ok "Telescope is installed and working"
else
    warn "Installation may have issues — check that the binary runs"
fi

# ── Done ──────────────────────────────────────────────

echo ""
printf "  \033[32mInstallation complete.\033[0m\n"
echo ""
printf "  \033[37mQuick reference:\033[0m\n"
printf "  \033[90m  tele service start    - start service + collectors\033[0m\n"
printf "  \033[90m  tele service status   - check service status\033[0m\n"
echo ""
printf "  \033[33mOpen a new terminal for PATH changes to take effect.\033[0m\n"
echo ""
