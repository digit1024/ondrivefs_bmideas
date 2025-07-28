#!/bin/bash
export RUST_LOG="debug"
export RUST_BACKTRACE=1
cargo test --test tests -- --test-threads=1 --nocapture
