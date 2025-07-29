#!/bin/bash
export RUST_LOG="info"
export RUST_BACKTRACE=1
cargo test --test tests -- --test-threads=1 --nocapture
