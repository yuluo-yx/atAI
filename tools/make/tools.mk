##@ 工具

TOOLS_BIN_DIR ?= $(HOME)/.local/bin
GOLANGCI_LINT_VERSION ?= v2.11.4
export PATH := $(TOOLS_BIN_DIR):$(PATH)

.PHONY: ensure-golangci-lint
ensure-golangci-lint: ## 检查 golangci-lint，缺失时自动安装到 TOOLS_BIN_DIR
	@$(LOG_TARGET)
	@set -euo pipefail; \
	if command -v golangci-lint >/dev/null 2>&1; then \
		printf '检测到 golangci-lint：%s\n' "$$(command -v golangci-lint)"; \
		golangci-lint --version; \
		exit 0; \
	fi; \
	script_path="$$(mktemp)"; \
	trap 'rm -f "$$script_path"' EXIT; \
	if command -v curl >/dev/null 2>&1; then \
		curl -sSfL https://golangci-lint.run/install.sh -o "$$script_path"; \
	elif command -v wget >/dev/null 2>&1; then \
		wget -qO "$$script_path" https://golangci-lint.run/install.sh; \
	else \
		printf '错误：未找到 curl 或 wget，无法自动安装 golangci-lint。\n' >&2; \
		exit 1; \
	fi; \
	mkdir -p "$(TOOLS_BIN_DIR)"; \
	printf '开始安装 golangci-lint %s 到 %s\n' "$(GOLANGCI_LINT_VERSION)" "$(TOOLS_BIN_DIR)"; \
	if ! /bin/sh "$$script_path" -b "$(TOOLS_BIN_DIR)" "$(GOLANGCI_LINT_VERSION)"; then \
		printf '错误：golangci-lint 官方安装流程执行失败，请检查网络连通性、版本号或目标目录权限。\n' >&2; \
		exit 1; \
	fi; \
	if ! command -v golangci-lint >/dev/null 2>&1; then \
		printf '错误：golangci-lint 安装完成后仍不可用，请检查 PATH 或目录权限。\n' >&2; \
		exit 1; \
	fi; \
	printf 'golangci-lint 安装完成：%s\n' "$$(command -v golangci-lint)"; \
	golangci-lint --version

.PHONY: golangci-lint-version
golangci-lint-version: ensure-golangci-lint ## 输出当前可用的 golangci-lint 版本
	@$(LOG_TARGET)
	@golangci-lint --version
