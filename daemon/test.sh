#!/bin/bash
export RUST_LOG="info"
export RUST_BACKTRACE=1

if [ $# -eq 1 ]; then
    # Run specific test
    cargo test "$1" -- --test-threads=1 --nocapture
else
    # Run all tests
    cargo test --test tests -- --test-threads=1 --nocapture
fi
