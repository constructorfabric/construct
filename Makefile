# `make help` lists the targets. cargo lives in ~/.cargo/bin.
export PATH := $(HOME)/.cargo/bin:$(PATH)

.DEFAULT_GOAL := help
.PHONY: help build test lint app-deps run package smoke

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

# The package declares the libraries Electron needs; apt installs them. The script runs as a normal
# user because the agent refuses to start PostgreSQL as root.
smoke: build app-deps ## Build the .deb, install it in a clean Ubuntu 24.04 container and start it (needs Docker)
	rm -rf app/out/make/deb
	cd app && npm run make
	docker run --rm -v "$(CURDIR)/app/out/make/deb/x64:/pkg:ro" -v "$(CURDIR)/scripts:/scripts:ro" ubuntu:24.04 bash -euc '\
		apt-get update -qq && \
		apt-get install -y -qq --no-install-recommends xvfb xauth /pkg/*.deb && \
		useradd --create-home smoke && \
		runuser -u smoke -- env HOME=/home/smoke /scripts/smoke-linux.sh'
