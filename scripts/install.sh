#!/usr/bin/env bash
# One-click HMIR installer for Linux and macOS
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/bhattkunalb/HMIR/main/scripts/install.sh | bash

set -euo pipefail

REPO="bhattkunalb/HMIR"
RELEASE_ENDPOINT="https://api.github.com/repos/${REPO}/releases/latest"
INSTALL_DIR="${HOME}/.local/bin"
LOCAL_BUILD=false

for arg in "$@"; do
  case "$arg" in
    --local) LOCAL_BUILD=true ;;
  esac
done

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1" >&2; }

check_prereqs() {
  for tool in curl tar git; do
    if ! command -v "${tool}" >/dev/null 2>&1; then
      log_error "${tool} is required but not installed."
      exit 1
    fi
  done
}

detect_platform() {
  local os arch
  os=$(uname -s | tr '[:upper:]' '[:lower:]')
  arch=$(uname -m)

  case "${os}" in
    linux) os="unknown-linux-gnu" ;;
    darwin) os="apple-darwin" ;;
    *) log_error "Unsupported OS: ${os}"; exit 1 ;;
  esac

  case "${arch}" in
    x86_64|amd64) arch="x86_64" ;;
    aarch64|arm64) arch="aarch64" ;;
    *) log_error "Unsupported architecture: ${arch}"; exit 1 ;;
  esac

  echo "${arch}-${os}"
}

build_from_source() {
  log_warn "Building HMIR from source (this may take 10-30 minutes)..."

  if ! command -v cargo >/dev/null 2>&1; then
    log_error "Rust toolchain required for source build. Install with https://rustup.rs/"
    exit 1
  fi

  local source_dir=""
  local tmp_repo=""

  if [[ "${LOCAL_BUILD}" == "true" ]] || [[ -f "./Cargo.toml" ]] || [[ -f "../Cargo.toml" ]]; then
    if [[ -f "./Cargo.toml" ]]; then
      source_dir=$(pwd)
    else
      source_dir=$(cd .. && pwd)
    fi
    log_info "Detected HMIR source at ${source_dir}. Building local version..."
  else
    tmp_repo=$(mktemp -d)
    log_info "Cloning repository to ${tmp_repo}..."
    git clone --depth 1 --branch main "https://github.com/${REPO}.git" "${tmp_repo}"
    source_dir="${tmp_repo}"
  fi

  (
    cd "${source_dir}"
    cargo build --release --workspace
    mkdir -p "${INSTALL_DIR}"
    cp target/release/hmir* "${INSTALL_DIR}/" 2>/dev/null || true
    chmod +x "${INSTALL_DIR}"/hmir* 2>/dev/null || true

    if [[ -d scripts ]]; then
      mkdir -p "${INSTALL_DIR}/scripts"
      cp -R scripts/. "${INSTALL_DIR}/scripts/"
    fi
  )

  if [[ -n "${tmp_repo}" ]]; then
    rm -rf "${tmp_repo}"
  fi

  log_info "Build complete. Binaries installed to ${INSTALL_DIR}"
}

install_binaries() {
  local platform=$1
  local tmp_dir
  local tag
  local asset_name
  local download_url

  if [[ "${LOCAL_BUILD}" == "true" ]]; then
    build_from_source
    return
  fi

  tmp_dir=$(mktemp -d)
  log_info "Fetching latest release from ${REPO}..."
  tag=$(curl -fsSL "${RELEASE_ENDPOINT}" | grep -o '"tag_name": *"[^"]*"' | head -1 | cut -d'"' -f4 || true)

  if [[ -z "${tag}" ]]; then
    log_warn "No release found. Falling back to source build."
    rm -rf "${tmp_dir}"
    build_from_source
    return
  fi

  asset_name="hmir-${tag}-${platform}.tar.gz"
  download_url="https://github.com/${REPO}/releases/download/${tag}/${asset_name}"
  log_info "Installing HMIR ${tag} for ${platform}..."

  if ! curl -fsSL -o "${tmp_dir}/${asset_name}" "${download_url}"; then
    log_warn "Prebuilt asset not found for ${platform}. Falling back to source build."
    rm -rf "${tmp_dir}"
    build_from_source
    return
  fi

  tar -xzf "${tmp_dir}/${asset_name}" -C "${tmp_dir}"
  mkdir -p "${INSTALL_DIR}"
  cp "${tmp_dir}"/hmir* "${INSTALL_DIR}/" 2>/dev/null || true
  chmod +x "${INSTALL_DIR}"/hmir* 2>/dev/null || true
  rm -rf "${tmp_dir}"

  log_info "Binaries installed to ${INSTALL_DIR}"
}

update_path() {
  if [[ ":$PATH:" == *":${INSTALL_DIR}:"* ]]; then
    return
  fi

  local shell_rc
  case "${SHELL:-}" in
    */zsh) shell_rc="${HOME}/.zshrc" ;;
    */bash) shell_rc="${HOME}/.bashrc" ;;
    *) shell_rc="${HOME}/.profile" ;;
  esac

  {
    echo
    echo "# HMIR installation"
    echo "export PATH=\"${INSTALL_DIR}:\$PATH\""
  } >> "${shell_rc}"

  log_info "Added ${INSTALL_DIR} to ${shell_rc}"
}

verify_install() {
  export PATH="${INSTALL_DIR}:${PATH}"
  if command -v hmir >/dev/null 2>&1; then
    log_info "HMIR installed: $(hmir --version 2>/dev/null || echo unknown)"
  else
    log_warn "hmir was installed, but your shell may need to be restarted."
  fi
}

main() {
  log_info "HMIR installer"
  log_info "Repository: https://github.com/${REPO}"
  log_info "HMIR auto-detects NPU, GPU, and CPU hardware after install."

  check_prereqs
  local platform
  platform=$(detect_platform)
  log_info "Detected platform: ${platform}"

  install_binaries "${platform}"
  update_path
  verify_install

  echo
  log_info "Installation complete."
  echo "Next steps:"
  echo "  1. Restart your shell or source your rc file."
  echo "  2. Run: hmir suggest"
  echo "  3. Start native dashboard: hmir start"
  echo "  4. Start legacy web API UI: hmir start --web"
  echo "  5. Integration help: hmir integrations"
}

main "$@"
