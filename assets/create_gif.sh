#!/bin/bash
# Generate the demo GIF for the README.
# Requires: vhs (https://github.com/charmbracelet/vhs), cargo
# Run from the repo root.

set -ex
cd "$(dirname "$0")/.."

# Build the project
cargo build --release

# Record the demo
vhs assets/demo.tape

echo "Demo GIF written to assets/demo.gif"
