#!/usr/bin/env bash
# scripts/install.sh
# One-click HMIR installer for Linux/macOS
# Usage: curl -fsSL https://raw.githubusercontent.com/bhattkunalb/HMIR/main/scripts/install.sh | sh

set -euo pipefail

# Configuration
REPO="bhattkunalb/HMIR"
RELEASE_ENDPOINT="https://api.github.com/repos/${REPO}/releases/latest"
INSTALL_DIR="${HOME}/.local/bin"
DASHBOARD_PORT=3001
API_PORT=8080

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1" >&2; }

# Check prerequisites
check_prereqs() {
  if ! command -v curl >/dev/null 2>&1; then
    log_error "curl is required but not installed. Please install curl and retry."
    exit 1
  fi
  
  if ! command -v tar >/dev/null 2>&1; then
    log_error "tar is required but not installed."
    exit 1
  fi
}

# Detect OS and architecture
detect_platform() {
  local os arch
  os=$(uname -s | tr '[:upper:]' '[:lower:]')
  arch=$(uname -m)
  
  case "$os" in
    darwin) os="apple-darwin" ;;
    linux) os="unknown-linux-gnu" ;;
    *) log_error "Unsupported OS: $os"; exit 1 ;;
  esac
  
  case "$arch" in
    x86_64|amd64) arch="x86_64" ;;
    aarch64|arm64) arch="aarch64" ;;
    *) log_error "Unsupported architecture: $arch"; exit 1 ;;
  esac
  
  echo "${arch}-${os}"
}

# Download and install binaries
install_binaries() {
  local platform=$1
  local tmp_dir
  tmp_dir=$(mktemp -d)
  
  log_info "Fetching latest release from ${REPO}..."
  
  # Get latest release tag
  local tag
  tag=$(curl -fsSL "${RELEASE_ENDPOINT}" | grep -o '"tag_name": *"[^"]*"' | head -1 | cut -d'"' -f4)
  
  if [[ -z "$tag" ]]; then
    log_warn "No releases found yet. Falling back to source build..."
    build_from_source
    return
  fi
  
  log_info "Installing HMIR ${tag} for ${platform}..."
  
  local asset_name="hmir-${tag}-${platform}.tar.gz"
  local download_url="https://github.com/${REPO}/releases/download/${tag}/${asset_name}"
  
  # Download and extract
  if ! curl -fsSL -o "${tmp_dir}/${asset_name}" "${download_url}"; then
    log_error "Failed to download ${asset_name}. It may not be built for your platform yet."
    log_warn "Fallback: building from source (requires Rust toolchain)..."
    build_from_source
    return
  fi
  
  tar -xzf "${tmp_dir}/${asset_name}" -C "${tmp_dir}"
  
  # Create install directory
  mkdir -p "${INSTALL_DIR}"
  
  # Install binaries
  cp "${tmp_dir}"/hmir-* "${INSTALL_DIR}/" 2>/dev/null || true
  chmod +x "${INSTALL_DIR}"/hmir-*
  
  # Cleanup
  rm -rf "${tmp_dir}"
  
  log_info "Binaries installed to ${INSTALL_DIR}"
}

# Fallback: build from source
build_from_source() {
  log_warn "Building HMIR from source (this may take 10-30 minutes)..."
  
  if ! command -v cargo >/dev/null 2>&1; then
    log_error "Rust toolchain required for source build. Install via: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
  fi
  
  local tmp_repo
  tmp_repo=$(mktemp -d)
  git clone --depth 1 --branch main "https://github.com/${REPO}.git" "${tmp_repo}"
  cd "${tmp_repo}"
  
  cargo build --release --workspace --features dashboard,openai-api,hardware-prober
  
  mkdir -p "${INSTALL_DIR}"
  cp target/release/hmir target/release/hmir-dashboard "${INSTALL_DIR}/" 2>/dev/null || true
  chmod +x "${INSTALL_DIR}"/hmir*
  
  cd - >/dev/null
  rm -rf "${tmp_repo}"
  
  log_info "Build complete. Binaries installed to ${INSTALL_DIR}"
}

# Update PATH if needed
update_path() {
  if [[ ":$PATH:" != *":${INSTALL_DIR}:"* ]]; then
    log_warn "${INSTALL_DIR} is not in your PATH."
    
    local shell_rc
    case "$SHELL" in
      */zsh) shell_rc="${HOME}/.zshrc" ;;
      */bash) shell_rc="${HOME}/.bashrc" ;;
      *) shell_rc="${HOME}/.profile" ;;
    esac
    
    echo "" >> "${shell_rc}"
    echo "# HMIR installation" >> "${shell_rc}"
    echo "export PATH=\"${INSTALL_DIR}:\$PATH\"" >> "${shell_rc}"
    
    log_info "Added ${INSTALL_DIR} to ${shell_rc}. Restart your shell or run: source ${shell_rc}"
  fi
}

# Post-install verification
verify_install() {
  log_info "Verifying installation..."
  
  if command -v hmir >/dev/null 2>&1; then
    local version
    version=$(hmir --version 2>/dev/null || echo "unknown")
    log_info "✅ HMIR installed: ${version}"
  else
    log_warn "⚠️  hmir command not found. Ensure ${INSTALL_DIR} is in your PATH."
  fi
}

# Main execution
main() {
  log_info "🚀 HMIR Installer"
  log_info "Repository: https://github.com/${REPO}"
  
  check_prereqs
  
  local platform
  platform=$(detect_platform)
  log_info "Detected platform: ${platform}"
  
  install_binaries "${platform}"
  update_path
  verify_install
  
  echo ""
  log_info "🎉 Installation complete!"
  echo ""
  echo "Next steps:"
  echo "  1. Restart your shell or run: source ~/.bashrc  # or ~/.zshrc"
  echo "  2. Get model recommendations: hmir suggest"
  echo "  3. Start the daemon: hmir start --dashboard"
  echo "  4. Open dashboard: http://localhost:${DASHBOARD_PORT}"
  echo "  5. API endpoint: http://localhost:${API_PORT}/v1/chat/completions"
  echo ""
  echo "Documentation: https://github.com/${REPO}/README.md"
}

main "$@"
