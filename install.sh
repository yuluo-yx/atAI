#!/usr/bin/env sh
set -eu

DEFAULT_REPO="yuluo-yx/atAI"

log() {
  printf '%s\n' "$*"
}

fail() {
  printf 'Error: %s\n' "$*" >&2
  exit 1
}

has_cmd() {
  command -v "$1" >/dev/null 2>&1
}

require_cmd() {
  has_cmd "$1" || fail "Missing required command: $1"
}

download_file() {
  url=$1
  output=$2

  if has_cmd curl; then
    curl -fsSL "$url" -o "$output"
    return
  fi

  if has_cmd wget; then
    wget -qO "$output" "$url"
    return
  fi

  fail "Missing download tool. Install curl or wget first."
}

download_text() {
  url=$1

  if has_cmd curl; then
    curl -fsSL "$url"
    return
  fi

  if has_cmd wget; then
    wget -qO- "$url"
    return
  fi

  fail "Missing download tool. Install curl or wget first."
}

resolve_target() {
  os_name=$(uname -s 2>/dev/null || true)
  arch_name=$(uname -m 2>/dev/null || true)

  case "${os_name}:${arch_name}" in
    Linux:x86_64 | Linux:amd64)
      printf 'x86_64-unknown-linux-gnu\n'
      ;;
    Darwin:x86_64 | Darwin:amd64)
      printf 'x86_64-apple-darwin\n'
      ;;
    Darwin:arm64 | Darwin:aarch64)
      printf 'aarch64-apple-darwin\n'
      ;;
    *)
      fail "Unsupported platform: ${os_name} ${arch_name}. Supported targets are Linux x86_64, macOS x86_64, and macOS arm64."
      ;;
  esac
}

resolve_release() {
  requested_version=$1

  if [ "$requested_version" = "latest" ]; then
    metadata=$(download_text "https://api.github.com/repos/${INSTALL_REPO}/releases/latest") || {
      fail "Failed to fetch latest release metadata"
    }

    release_tag=$(printf '%s\n' "$metadata" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)
    [ -n "$release_tag" ] || fail "Failed to parse tag_name from GitHub release metadata"
    release_version=${release_tag#v}
  else
    case "$requested_version" in
      v*)
        release_tag=$requested_version
        release_version=${requested_version#v}
        ;;
      *)
        release_tag="v${requested_version}"
        release_version=$requested_version
        ;;
    esac
  fi
}

path_contains() {
  target_dir=$1
  case ":${PATH:-}:" in
    *:"$target_dir":*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

[ -n "${HOME:-}" ] || fail "HOME environment variable is not set"
require_cmd mktemp
require_cmd sed

INSTALL_REPO=${ATAI_INSTALL_REPO:-$DEFAULT_REPO}
INSTALL_DIR=${ATAI_INSTALL_DIR:-"$HOME/.local/bin"}
REQUESTED_VERSION=${ATAI_INSTALL_VERSION:-latest}
RUNTIME_DIR="$HOME/.@ai"

TARGET=$(resolve_target)
resolve_release "$REQUESTED_VERSION"

BINARY_NAME="atai-${release_version}-${TARGET}"
BINARY_URL="https://github.com/${INSTALL_REPO}/releases/download/${release_tag}/${BINARY_NAME}"
WRAPPER_URL="https://raw.githubusercontent.com/${INSTALL_REPO}/${release_tag}/scripts/@ai"
TMP_DIR=$(mktemp -d 2>/dev/null || mktemp -d -t atai-install)
BINARY_PATH="${TMP_DIR}/atai"
WRAPPER_PATH="${TMP_DIR}/@ai"
LEGACY_ARCHIVE_NAME="atai-${release_version}-${TARGET}.tar.gz"
LEGACY_ARCHIVE_URL="https://github.com/${INSTALL_REPO}/releases/download/${release_tag}/${LEGACY_ARCHIVE_NAME}"
LEGACY_ARCHIVE_PATH="${TMP_DIR}/${LEGACY_ARCHIVE_NAME}"
EXTRACT_DIR="${TMP_DIR}/extract"
PACKAGE_DIR="${EXTRACT_DIR}/atai-${release_version}-${TARGET}"

cleanup() {
  if [ -n "${TMP_DIR:-}" ] && [ -d "$TMP_DIR" ]; then
    rm -rf "$TMP_DIR"
  fi
}

trap cleanup 0 HUP INT TERM

log "Installing atai ${release_version} (${TARGET})"
log "Binary URL: ${BINARY_URL}"
log "Wrapper URL: ${WRAPPER_URL}"

if download_file "$BINARY_URL" "$BINARY_PATH"; then
  binary_source="release binary"
else
  require_cmd tar
  mkdir -p "$EXTRACT_DIR"
  if ! download_file "$LEGACY_ARCHIVE_URL" "$LEGACY_ARCHIVE_PATH"; then
    fail "Failed to download the release binary. Verify the version, platform target, and release assets."
  fi

  tar -xzf "$LEGACY_ARCHIVE_PATH" -C "$EXTRACT_DIR" || fail "Failed to extract legacy archive: $LEGACY_ARCHIVE_PATH"
  [ -d "$PACKAGE_DIR" ] || fail "Unexpected legacy archive layout: $PACKAGE_DIR"
  cp "$PACKAGE_DIR/atai" "$BINARY_PATH"
  binary_source="legacy release archive"
fi

download_file "$WRAPPER_URL" "$WRAPPER_PATH" || fail "Failed to download the @ai wrapper script from the tagged source tree"

mkdir -p "$INSTALL_DIR"
cp "$BINARY_PATH" "$INSTALL_DIR/atai"
cp "$WRAPPER_PATH" "$INSTALL_DIR/@ai"
chmod +x "$INSTALL_DIR/atai" "$INSTALL_DIR/@ai"

needs_init=0
for required_file in config.toml system_prompt.txt command_denylist.txt command_confirmlist.txt; do
  if [ ! -f "$RUNTIME_DIR/$required_file" ]; then
    needs_init=1
    break
  fi
done

if [ "$needs_init" -eq 1 ]; then
  log "Missing runtime resources detected, running initialization"
  if ! "$INSTALL_DIR/atai" config init; then
    fail "atai was installed, but initialization failed. Run this command manually: $INSTALL_DIR/atai config init"
  fi
  init_status="Ran atai config init"
else
  init_status="Existing runtime configuration detected, initialization skipped"
fi

log "Installation completed"
log "Version: ${release_version}"
log "Directory: ${INSTALL_DIR}"
log "Binary source: ${binary_source}"
log "$init_status"

if ! path_contains "$INSTALL_DIR"; then
  log "Notice: ${INSTALL_DIR} is not in PATH"
  log "Run: export PATH=\"${INSTALL_DIR}:\$PATH\""
fi
