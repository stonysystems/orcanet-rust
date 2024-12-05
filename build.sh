#!/bin/bash
RED="\033[31m"
YELLOW="\033[33m"
GREEN="\033[32m"
RESET="\033[0m"

set -e # Exit on error. We need all of these commands to succeed.

# Install sqlite3 if not present
echo "${YELLOW}=> Check for SQLite:${RESET}"

if [[ "$OSTYPE" == "darwin"* ]]; then  # MacOS
    if ! command -v sqlite3 &> /dev/null; then
        echo "${YELLOW}Installing  SQLite...${RESET}"
        brew install sqlite3 libsqlite3-dev
    else
        echo "${GREEN}SQLite already installed. Skipped.${GREEN}"
    fi
elif [[ "$OSTYPE" == "linux-gnu"* ]]; then # Linux
    sudo apt-get update
    if ! command -v sqlite3 &> /dev/null; then
        echo "${YELLOW}Installing  SQLite...!${RESET}"
        sudo apt-get install -y sqlite3
    else
        echo "${GREEN}SQLite already installed. Skipped.${GREEN}"
    fi
fi

# Install Rust if rustc or cargo is not present
echo "\\n${YELLOW}=> Check for Rust:${RESET}"

if ! command -v rustc &> /dev/null || ! command -v cargo &> /dev/null; then
    echo "${YELLOW}Installing  Rust...${RESET}"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    . "$HOME/.cargo/env"
else
    echo "${GREEN}Rust already installed. Skipped.${GREEN}"
fi

# Verify installations
echo "\\n${YELLOW}=> Verify installations:${RESET}"
sqlite3 --version
rustc --version
cargo --version

# Switch to nightly rust
apt install libssl-dev
echo "\\n${YELLOW}=> Switch to nightly rust"
rustup default nightly >> /dev/null
echo "${GREEN}Switched to nightly rust${RESET}"

# Build the code
echo "\\n${YELLOW}=> Build:"
RUSTFLAGS="-Awarnings" RUST_LOG=info cargo build --release
echo "${GREEN}Build complete${RESET}. The binary should be in ${YELLOW}$(pwd)/target/release${RESET}"