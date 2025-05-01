format:
	cargo +nightly fmt --all
	cargo sort --workspace --grouped

lint:
	cargo +nightly fmt --all -- --check
	cargo clippy --all -- -D warnings
	cargo sort --check --workspace --grouped
	cargo machete