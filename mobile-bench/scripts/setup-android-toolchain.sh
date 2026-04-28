#!/usr/bin/env bash
# This file is part of midnight-ledger.
# Copyright (C) Midnight Foundation
# SPDX-License-Identifier: Apache-2.0
#
# One-shot toolchain setup for building the Dioxus-based proof bench app
# (mobile-bench/dioxus-bench) for desktop + Android (emulator and device).
#
# Tested on: macOS 14+ (Apple Silicon).  Re-runnable: each step skips if
# the requested artifact is already present.
#
# Usage:
#   bash mobile-bench/scripts/setup-android-toolchain.sh
#
# After it finishes, follow the printed instructions to add a few env vars
# to your shell rc file.

set -euo pipefail

# ----------------------------------------------------------------------------
# Configuration — bump these when you want a newer toolchain.
# ----------------------------------------------------------------------------
NDK_VERSION="27.2.12479018"
ANDROID_PLATFORM="android-34"
BUILD_TOOLS_VERSION="34.0.0"
SYSTEM_IMAGE="system-images;${ANDROID_PLATFORM};google_apis;arm64-v8a"
JDK_CASK="temurin@17"
DIOXUS_CLI_VERSION="0.6.3"     # pin to a known-good Dioxus

# ----------------------------------------------------------------------------
# Helpers
# ----------------------------------------------------------------------------
log()  { printf '\033[1;34m[setup]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[warn ]\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31m[fail ]\033[0m %s\n' "$*" >&2; exit 1; }

require_macos() {
  [ "$(uname -s)" = "Darwin" ] || die "This script targets macOS. Detected: $(uname -s)"
}

ensure_brew() {
  command -v brew >/dev/null 2>&1 || die "Homebrew is required. Install from https://brew.sh first."
}

brew_install() {
  local kind="$1" pkg="$2"
  if [ "$kind" = "cask" ]; then
    if brew list --cask --versions "$pkg" >/dev/null 2>&1; then
      log "brew cask $pkg — already installed"
    else
      log "brew install --cask $pkg"
      brew install --cask "$pkg"
    fi
  else
    if brew list --versions "$pkg" >/dev/null 2>&1; then
      log "brew $pkg — already installed"
    else
      log "brew install $pkg"
      brew install "$pkg"
    fi
  fi
}

cargo_install_if_missing() {
  local bin="$1" crate="$2" version="${3:-}"
  if command -v "$bin" >/dev/null 2>&1; then
    log "$bin — already installed ($("$bin" --version 2>/dev/null | head -n1))"
    return 0
  fi
  if [ -n "$version" ]; then
    log "cargo install $crate --version $version --locked"
    cargo install "$crate" --version "$version" --locked
  else
    log "cargo install $crate --locked"
    cargo install "$crate" --locked
  fi
}

rustup_target_add_if_missing() {
  local target="$1"
  if rustup target list --installed | grep -qx "$target"; then
    log "rust target $target — already installed"
  else
    log "rustup target add $target"
    rustup target add "$target"
  fi
}

# ----------------------------------------------------------------------------
# Step 1 — preflight
# ----------------------------------------------------------------------------
require_macos
ensure_brew
command -v rustup >/dev/null 2>&1 || die "rustup is required. Install from https://rustup.rs first."
command -v cargo  >/dev/null 2>&1 || die "cargo not found on PATH."

# ----------------------------------------------------------------------------
# Step 2 — JDK 17 (needed by the Android Gradle wrapper that Dioxus invokes)
# ----------------------------------------------------------------------------
brew_install cask "$JDK_CASK"

# Find JAVA_HOME for the installed Temurin 17 (used later in the env-var
# instructions and exported for the sdkmanager calls below).
JAVA_HOME_PATH="$(/usr/libexec/java_home -v 17 2>/dev/null || true)"
[ -n "$JAVA_HOME_PATH" ] || die "JDK 17 install completed but /usr/libexec/java_home -v 17 returned nothing."
export JAVA_HOME="$JAVA_HOME_PATH"
log "JAVA_HOME=$JAVA_HOME"

# ----------------------------------------------------------------------------
# Step 3 — Android command-line tools (sdkmanager, avdmanager)
# ----------------------------------------------------------------------------
brew_install cask android-commandlinetools

# Homebrew installs the cmdline tools to /opt/homebrew/share/android-commandlinetools
# (Apple Silicon) or /usr/local/share/android-commandlinetools (Intel).
BREW_PREFIX="$(brew --prefix)"
ANDROID_HOME_DEFAULT="$BREW_PREFIX/share/android-commandlinetools"
ANDROID_HOME="${ANDROID_HOME:-$ANDROID_HOME_DEFAULT}"
[ -d "$ANDROID_HOME" ] || die "Expected ANDROID_HOME at $ANDROID_HOME but it does not exist."

SDKMANAGER="$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager"
[ -x "$SDKMANAGER" ] || die "sdkmanager not found at $SDKMANAGER (cask layout changed?)"

export ANDROID_HOME
log "ANDROID_HOME=$ANDROID_HOME"

# ----------------------------------------------------------------------------
# Step 4 — accept SDK licenses (idempotent)
# ----------------------------------------------------------------------------
log "Accepting SDK licenses (yes-piped, safe to re-run)…"
yes | "$SDKMANAGER" --licenses >/dev/null

# ----------------------------------------------------------------------------
# Step 5 — Android NDK + platform + build-tools + emulator image
# ----------------------------------------------------------------------------
log "Installing Android packages (NDK, platform-$ANDROID_PLATFORM, build-tools, arm64 system image)…"
"$SDKMANAGER" \
  "platform-tools" \
  "platforms;${ANDROID_PLATFORM}" \
  "build-tools;${BUILD_TOOLS_VERSION}" \
  "ndk;${NDK_VERSION}" \
  "emulator" \
  "$SYSTEM_IMAGE"

ANDROID_NDK_HOME="$ANDROID_HOME/ndk/$NDK_VERSION"
[ -d "$ANDROID_NDK_HOME" ] || die "NDK install reported success but $ANDROID_NDK_HOME is missing."
log "ANDROID_NDK_HOME=$ANDROID_NDK_HOME"

# ----------------------------------------------------------------------------
# Step 6 — create a default AVD if one with our name doesn't exist
# ----------------------------------------------------------------------------
AVDMANAGER="$ANDROID_HOME/cmdline-tools/latest/bin/avdmanager"
AVD_NAME="midnight_bench_arm64_api34"
if "$AVDMANAGER" list avd 2>/dev/null | grep -q "Name: $AVD_NAME"; then
  log "AVD '$AVD_NAME' — already exists"
else
  log "Creating AVD '$AVD_NAME' (arm64-v8a, API 34)…"
  echo "no" | "$AVDMANAGER" create avd \
    --name "$AVD_NAME" \
    --package "$SYSTEM_IMAGE" \
    --device "pixel_7"
fi

# ----------------------------------------------------------------------------
# Step 7 — Rust targets
# ----------------------------------------------------------------------------
# arm64 covers both the recommended emulator image AND real arm64 devices
# (S24 Ultra). armv7 is kept for older 32-bit devices; harmless to add.
rustup_target_add_if_missing aarch64-linux-android
rustup_target_add_if_missing armv7-linux-androideabi
# x86_64 only needed if you ever use an x86_64 emulator image. Off by default
# to keep the toolchain lean.
# rustup_target_add_if_missing x86_64-linux-android

# ----------------------------------------------------------------------------
# Step 8 — cargo-ndk + dioxus-cli
# ----------------------------------------------------------------------------
cargo_install_if_missing cargo-ndk cargo-ndk
cargo_install_if_missing dx        dioxus-cli "$DIOXUS_CLI_VERSION"

# ----------------------------------------------------------------------------
# Step 9 — print env-var instructions
# ----------------------------------------------------------------------------
cat <<EOF

================================================================================
Toolchain install complete.

Add the following to your shell rc (~/.zshrc):

    export JAVA_HOME="\$(/usr/libexec/java_home -v 17)"
    export ANDROID_HOME="$ANDROID_HOME"
    export ANDROID_NDK_HOME="\$ANDROID_HOME/ndk/$NDK_VERSION"
    export PATH="\$ANDROID_HOME/cmdline-tools/latest/bin:\$ANDROID_HOME/platform-tools:\$ANDROID_HOME/emulator:\$PATH"

Then 'source ~/.zshrc' (or open a new shell) and verify:

    rustup target list --installed | grep android
    cargo ndk --version
    dx --version
    sdkmanager --list_installed | head
    avdmanager list avd | grep $AVD_NAME

To launch the emulator:
    emulator -avd $AVD_NAME &

To list connected devices (emulator + USB):
    adb devices
================================================================================
EOF
