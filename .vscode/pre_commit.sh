#!/bin/sh

set -e

cargo +nightly fmt --all -- --check
cargo lint
cargo test
cargo sort-deps
cargo-machete