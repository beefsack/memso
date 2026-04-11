#!/usr/bin/env bash
# Dev channel bootstrap for the memso-dev Claude Code plugin.
# Runs on SessionStart. Fetches a lightweight SHA file from the rolling 'dev'
# GitHub Release and only downloads the binary when the SHA has changed.
# Always exits 0 so a failed download does not block the session.
set -uo pipefail

BINARY="${CLAUDE_PLUGIN_DATA}/memso"
VERSION_FILE="${CLAUDE_PLUGIN_DATA}/version"
REPO="beefsack/memso"
DEV_BASE_URL="https://github.com/${REPO}/releases/download/dev"

# Allow an explicit binary override - useful in development or for custom builds.
# Set MEMSO_BINARY_OVERRIDE to an absolute path to skip the GitHub download entirely.
if [[ -n "${MEMSO_BINARY_OVERRIDE:-}" ]]; then
  if [[ ! -f "${MEMSO_BINARY_OVERRIDE}" ]]; then
    echo "[memso] MEMSO_BINARY_OVERRIDE is set but '${MEMSO_BINARY_OVERRIDE}' does not exist" >&2
    exit 0
  fi
  mkdir -p "${CLAUDE_PLUGIN_DATA}"
  cp -f "${MEMSO_BINARY_OVERRIDE}" "${BINARY}"
  chmod +x "${BINARY}"
  echo "[memso bootstrap] Binary ready (dev override: ${MEMSO_BINARY_OVERRIDE})"
  exit 0
fi

# Detect OS and architecture.
OS="$(uname -s)"
ARCH="$(uname -m)"

case "${OS}" in
  Linux)
    case "${ARCH}" in
      x86_64)          ARTIFACT="memso-linux-x86_64.tar.gz" ;;
      aarch64 | arm64) ARTIFACT="memso-linux-aarch64.tar.gz" ;;
      *) echo "[memso] Unsupported architecture: ${ARCH}" >&2; exit 0 ;;
    esac
    ;;
  Darwin)
    case "${ARCH}" in
      arm64)  ARTIFACT="memso-macos-aarch64.tar.gz" ;;
      x86_64) ARTIFACT="memso-macos-x86_64.tar.gz" ;;
      *) echo "[memso] Unsupported architecture: macOS ${ARCH}" >&2; exit 0 ;;
    esac
    ;;
  MINGW* | MSYS* | CYGWIN*)
    # Windows via Git Bash - Git Bash is required for Claude Code hooks on Windows.
    ARTIFACT="memso-windows-x86_64.zip"
    BINARY="${BINARY}.exe"
    ;;
  *)
    echo "[memso] Unsupported OS: ${OS}" >&2
    exit 0
    ;;
esac

# Fetch the remote SHA to check whether the dev build has changed since last install.
# Uses a tiny text file so the common case (no update) costs one small HTTP request.
REMOTE_SHA="$(curl -fsSL "${DEV_BASE_URL}/dev-version.txt" 2>/dev/null || true)"
if [[ -z "${REMOTE_SHA}" ]]; then
  echo "[memso] Could not reach dev release - using existing binary if available" >&2
  echo "[memso bootstrap] Binary ready (offline, SHA unknown)"
  exit 0
fi

LOCAL_SHA="$(cat "${VERSION_FILE}" 2>/dev/null || echo "")"
if [[ "${REMOTE_SHA}" == "${LOCAL_SHA}" && -f "${BINARY}" ]]; then
  echo "[memso bootstrap] Binary ready (dev ${REMOTE_SHA:0:7})"
  exit 0
fi

# Download and extract the updated dev binary.
echo "[memso] Downloading dev build ${REMOTE_SHA:0:7}..." >&2

mkdir -p "${CLAUDE_PLUGIN_DATA}"
ARCHIVE="${CLAUDE_PLUGIN_DATA}/${ARTIFACT}"

if ! curl -fsSL "${DEV_BASE_URL}/${ARTIFACT}" -o "${ARCHIVE}"; then
  echo "[memso] Download failed: ${DEV_BASE_URL}/${ARTIFACT}" >&2
  exit 0
fi

case "${ARTIFACT}" in
  *.tar.gz)
    tar xzf "${ARCHIVE}" -C "${CLAUDE_PLUGIN_DATA}"
    rm -f "${ARCHIVE}"
    chmod +x "${BINARY}"
    ;;
  *.zip)
    unzip -o "${ARCHIVE}" -d "${CLAUDE_PLUGIN_DATA}"
    rm -f "${ARCHIVE}"
    ;;
esac

printf '%s' "${REMOTE_SHA}" > "${VERSION_FILE}"

echo "[memso] Installed dev build ${REMOTE_SHA:0:7} to ${BINARY}" >&2
echo "[memso bootstrap] Downloaded and installed dev build ${REMOTE_SHA:0:7} (${OS}/${ARCH})"
