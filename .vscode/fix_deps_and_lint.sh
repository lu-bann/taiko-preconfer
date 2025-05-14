#!/bin/sh

set -e

cargo +nightly fmt --all
cargo lint-fix
cargo sort-deps-fix
