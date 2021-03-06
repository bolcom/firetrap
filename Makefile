.PHONY: help
help: # Shows available `make` commands
	@echo 'Available `make` commands:' >/dev/stderr
	@echo >/dev/stderr
	@awk -F'#' '/^[a-z][A-Za-z0-9]+/ {if (NF > 1) { sub(/:[^#]*/, ""); print $$1 "\t\t" $$2}}' Makefile

.PHONY: test
test: # Runs unit and integration tests
	cargo test

.PHONY: docs
docs: # Creates the API docs and opens it in the browser
	cargo doc --no-deps --open

.PHONY: examples
examples:
	cargo build --examples

.PHONY: build
build: # Creates a release build
	cargo build --release

.PHONY: pr-prep
pr-prep: # Runs checks to ensure you're ready for a pull request
	cargo fmt --all -- --check
	cargo build --examples --workspace
	cargo build  --workspace
	cargo clippy  --workspace
	cargo test  --workspace
	cargo test --doc --workspace
	cargo doc --workspace --no-deps

.PHONY: publish
publish: # Publishes the lib to crates.io
	cargo publish --verbose
