#!/bin/sh

set -e

cargo llvm-cov --lcov --output-path lcov.info --ignore-filename-regex taiko_anchor.rs
