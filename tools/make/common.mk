SHELL := /bin/bash

ROOT_DIR := $(CURDIR)
BINARY_NAME := atai
INSTALL_DIR ?= $(HOME)/.local/bin
BUILD_PROFILE ?= release
BUILD_TARGET ?=
BUILD_ALL_TARGETS ?= x86_64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-msvc
RUN_ARGS ?=
VERSION ?= $(shell awk 'BEGIN{in_section=0} /^\[workspace.package\]/{in_section=1; next} /^\[/{if(in_section) exit} in_section && /^version = /{gsub(/^version = "|"/, "", $$0); print $$0; exit}' Cargo.toml)
BUILD_PROFILE_DIR := $(if $(filter $(BUILD_PROFILE),release),release,debug)
CARGO_BUILD_PROFILE_FLAG := $(if $(filter $(BUILD_PROFILE),release),--release,)
BUILD_TARGET_FLAG := $(if $(strip $(BUILD_TARGET)),--target $(BUILD_TARGET),)
BUILD_OUTPUT_DIR := $(if $(strip $(BUILD_TARGET)),target/$(BUILD_TARGET)/$(BUILD_PROFILE_DIR),target/$(BUILD_PROFILE_DIR))
HOST_OS := $(if $(filter undefined,$(origin OS)),$(shell uname -s 2>/dev/null || echo Unknown),$(OS))
HOST_BINARY_SUFFIX := $(if $(filter Windows_NT,$(HOST_OS)),.exe,)
INSTALL_BINARY_PATH := target/release/$(BINARY_NAME)$(HOST_BINARY_SUFFIX)

LOG_TARGET = echo -e "\033[0;32m==================> Running $@ ============> ... \033[0m"

.PHONY: help
help:
	@echo -e "\033[1;3;34m @ai, AI-assisted shell command review tool.\033[0m\n"
	@echo -e "Usage:\n  make \033[36m<Target>\033[0m \033[36m<Option>\033[0m\n\nTargets:"
	@awk 'BEGIN {FS = ":.*##"; printf ""} /^[a-zA-Z_0-9-]+:.*?##/ { printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2 } /^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) } ' $(MAKEFILE_LIST)
