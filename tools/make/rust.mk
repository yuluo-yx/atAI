##@ build

CARGO := cargo
TARGET_DIR := target/$(BUILD_PROFILE)

.PHONY: build
build: ## Build the binary for the selected profile
	@$(LOG_TARGET)
	$(CARGO) build -p $(BINARY_NAME) $(if $(filter $(BUILD_PROFILE),release),--release,)

.PHONY: run
run: ## Run the CLI locally, e.g. make run RUN_ARGS='find the largest directories'
	@$(LOG_TARGET)
	$(CARGO) run -p $(BINARY_NAME) -- $(RUN_ARGS)

.PHONY: install
install: ## Install atai and the @ai wrapper to $(INSTALL_DIR)
	@$(LOG_TARGET)
	$(CARGO) build -p $(BINARY_NAME) --release
	mkdir -p "$(INSTALL_DIR)"
	cp "$(TARGET_DIR)/$(BINARY_NAME)" "$(INSTALL_DIR)/$(BINARY_NAME)"
	cp "scripts/@ai" "$(INSTALL_DIR)/@ai"
	chmod +x "$(INSTALL_DIR)/$(BINARY_NAME)" "$(INSTALL_DIR)/@ai"
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
