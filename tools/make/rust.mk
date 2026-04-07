##@ build

CARGO := cargo

.PHONY: build
build: ## Build the binary; set BUILD_TARGET=<triple> for cross builds
	@$(LOG_TARGET)
	$(CARGO) build -p $(BINARY_NAME) $(CARGO_BUILD_PROFILE_FLAG) $(BUILD_TARGET_FLAG)

.PHONY: build-all
build-all: ## Build every target from the release matrix and fail on the first missing dependency
	@$(LOG_TARGET)
	@if ! command -v rustup >/dev/null 2>&1; then \
		echo "Missing rustup. Install rustup before running make build-all." >&2; \
		exit 1; \
	fi
	@set -euo pipefail; \
	for target in $(BUILD_ALL_TARGETS); do \
		echo "==> Building $$target"; \
		if ! rustup target list --installed | grep -qx "$$target"; then \
			echo "Missing Rust target $$target. Run: rustup target add $$target" >&2; \
			exit 1; \
		fi; \
		if ! $(CARGO) build -p $(BINARY_NAME) $(CARGO_BUILD_PROFILE_FLAG) --target "$$target"; then \
			case "$$target" in \
				x86_64-pc-windows-msvc) \
					echo "Failed to build $$target. This target requires the MSVC toolchain and is typically built on a Windows host." >&2 ;; \
				*apple-darwin) \
					echo "Failed to build $$target. Ensure Xcode Command Line Tools and the Apple SDK are available." >&2 ;; \
				*) \
					echo "Failed to build $$target. Ensure the linker and C toolchain for $$target are installed." >&2 ;; \
			esac; \
			exit 1; \
		fi; \
	done

.PHONY: run
run: ## Run the CLI locally, e.g. make run RUN_ARGS='find the largest directories'
	@$(LOG_TARGET)
	$(CARGO) run -p $(BINARY_NAME) -- $(RUN_ARGS)

.PHONY: install
install: ## Install the host binary and the @ai wrapper to $(INSTALL_DIR)
	@$(LOG_TARGET)
	$(CARGO) build -p $(BINARY_NAME) --release
	mkdir -p "$(INSTALL_DIR)"
	cp "$(INSTALL_BINARY_PATH)" "$(INSTALL_DIR)/$(BINARY_NAME)$(HOST_BINARY_SUFFIX)"
	cp "scripts/@ai" "$(INSTALL_DIR)/@ai"
	chmod +x "$(INSTALL_DIR)/$(BINARY_NAME)$(HOST_BINARY_SUFFIX)" "$(INSTALL_DIR)/@ai"
	@printf 'Installed %s %s to %s\n' "$(BINARY_NAME)" "$(VERSION)" "$(INSTALL_DIR)"

##@ quality

.PHONY: fmt
fmt: ## Format Rust source code
	@$(LOG_TARGET)
	$(CARGO) fmt --all

.PHONY: fmt-check
fmt-check: ## Check Rust formatting without rewriting files
	@$(LOG_TARGET)
	$(CARGO) fmt --all -- --check

.PHONY: check
check: ## Run cargo check for all targets and features
	@$(LOG_TARGET)
	$(CARGO) check --workspace --all-targets --all-features

.PHONY: test
test: ## Run unit tests
	@$(LOG_TARGET)
	$(CARGO) test --workspace

.PHONY: clippy
clippy: ## Run clippy with warnings treated as errors
	@$(LOG_TARGET)
	$(CARGO) clippy --workspace --all-targets --all-features -- -D warnings

.PHONY: verify
verify: ## Run formatting check, cargo check, clippy and tests
verify: fmt-check check clippy test

##@ housekeeping

.PHONY: clean
clean: ## Clean build artifacts
	@$(LOG_TARGET)
	$(CARGO) clean
