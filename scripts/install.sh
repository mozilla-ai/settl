#!/bin/bash
set -e

REPO="Brake-Labs/settl"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
BINARY_NAME="settl"

info() { printf "\033[34m[info]\033[0m %s\n" "$1"; }
success() { printf "\033[32m[ok]\033[0m %s\n" "$1"; }
error() { printf "\033[31m[error]\033[0m %s\n" "$1" >&2; exit 1; }

detect_platform() {
    local os arch
    os=$(uname -s | tr '[:upper:]' '[:lower:]')
    arch=$(uname -m)

    case "$os" in
        linux) os="linux" ;;
        darwin) os="darwin" ;;
        *) error "Unsupported OS: $os" ;;
    esac

    case "$arch" in
        x86_64|amd64) arch="amd64" ;;
        aarch64|arm64) arch="arm64" ;;
        *) error "Unsupported architecture: $arch" ;;
    esac

    echo "${os}-${arch}"
}

get_latest_version() {
    curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
        | grep '"tag_name"' \
        | sed -E 's/.*"([^"]+)".*/\1/'
}

main() {
    info "Detecting platform..."
    platform=$(detect_platform)
    success "Platform: $platform"

    info "Fetching latest version..."
    version=$(get_latest_version)
    if [ -z "$version" ]; then
        error "Failed to fetch latest version"
    fi
    success "Latest version: $version"

    download_url="https://github.com/${REPO}/releases/download/${version}/settl-${platform}.tar.gz"
    info "Downloading from: $download_url"

    tmp_dir=$(mktemp -d)
    trap 'rm -rf "$tmp_dir"' EXIT

    curl -fsSL "$download_url" -o "$tmp_dir/settl.tar.gz" || error "Download failed"
    success "Downloaded successfully"

    info "Extracting..."
    tar xzf "$tmp_dir/settl.tar.gz" -C "$tmp_dir"

    info "Installing to $INSTALL_DIR..."
    if [ -w "$INSTALL_DIR" ]; then
        mv "$tmp_dir/settl-${platform}" "$INSTALL_DIR/$BINARY_NAME"
    else
        sudo mv "$tmp_dir/settl-${platform}" "$INSTALL_DIR/$BINARY_NAME"
    fi
    chmod +x "$INSTALL_DIR/$BINARY_NAME"

    success "Installed $BINARY_NAME $version to $INSTALL_DIR/$BINARY_NAME"
    echo ""
    success "Run 'settl' to get started!"
}

main "$@"
