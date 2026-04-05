SHELL := /bin/bash

ROOT_DIR := $(CURDIR)
BINARY_NAME := atai
INSTALL_DIR ?= $(HOME)/.local/bin
BUILD_PROFILE ?= release
RUN_ARGS ?=
VERSION ?= $(shell awk 'BEGIN{in_section=0} /^\[workspace.package\]/{in_section=1; next} /^\[/{if(in_section) exit} in_section && /^version = /{gsub(/^version = "|"/, "", $$0); print $$0; exit}' Cargo.toml)

LOG_TARGET = echo -e "\033[0;32m==================> Running $@ ============> ... \033[0m"

.PHONY: help
help:
	@echo -e "\033[1;3;34m @ai, AI-assisted shell command review tool.\033[0m\n"
	@echo -e "Usage:\n  make \033[36m<Target>\033[0m \033[36m<Option>\033[0m\n\nTargets:"
	@awk 'BEGIN {FS = ":.*##"; printf ""} /^[a-zA-Z_0-9-]+:.*?##/ { printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2 } /^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) } ' $(MAKEFILE_LIST)
