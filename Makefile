# `make help` lists the targets. cargo lives in ~/.cargo/bin.
export PATH := $(HOME)/.cargo/bin:$(PATH)

.DEFAULT_GOAL := help
.PHONY: help build test

help: ## List the targets
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F':.*## ' '{printf "  %-10s %s\n", $$1, $$2}'

build: ## Build the agent: target/release/construct-agent
	cargo build --release --locked

test: ## Checks (fmt, clippy, tests)
	cargo fmt --check
	cargo clippy --all-targets --locked -- -D warnings
	cargo test --locked
