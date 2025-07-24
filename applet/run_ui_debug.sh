#!/usr/bin/env bash
# Run the UI with full Rust backtraces and INFO-level logging

set -e
export RUST_BACKTRACE=1
export RUST_LOG=info

cd "$(dirname "$0")"
cargo run -- 