.PHONY: run fmt clippy lint test docker-build docker-run

run:
	cargo run --all-features

fmt:
	cargo fmt --all -- --check

clippy:
	cargo clippy --all-targets --all-features

lint: fmt clippy

test:
	cargo test --all-features

docker-build:
	docker build -t utxone-rust .

docker-run: docker-build
	docker run -p 18018:18018 utxone-rust
