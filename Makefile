# `make help` lists the targets. cargo lives in ~/.cargo/bin.
export PATH := $(HOME)/.cargo/bin:$(PATH)

.DEFAULT_GOAL := help
.PHONY: help build test lint app-deps run package

help: ## List the targets
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F':.*## ' '{printf "  %-10s %s\n", $$1, $$2}'

build: ## Build the agent: target/release/construct-agent
	cargo build --release --locked

test: build ## Agent checks (fmt, clippy, tests), then the app tests
	cargo fmt --check
	cargo clippy --all-targets --locked -- -D warnings
	cargo test --locked
	cd app && xvfb-run -a npm test

lint: ## App lint and type check
	cd app && npm run lint && npm run lint:ts

# npm skips the Electron download script, so it runs by hand.
app/node_modules: app/package-lock.json
	cd app && npm ci && node node_modules/electron/install.js
	@touch app/node_modules

app-deps: app/node_modules ## Install the app's dependencies

run: build app-deps ## Build the agent, then start the app in development mode
	cd app && npm start

package: build app-deps ## Package the app with the agent inside
	cd app && npm run package
