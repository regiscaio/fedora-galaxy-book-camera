SHELL := /usr/bin/env bash

APP_ID := com.caioregis.GalaxyBookCamera
BIN := galaxybook-camera
PACKAGE_NAME := galaxybook-camera
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml)
IMAGE_NAME := localhost/galaxybook-camera-builder:fedora44
CACHE_ROOT ?= $(HOME)/.cache/galaxybook-camera-rust
CARGO_REGISTRY_CACHE := $(CACHE_ROOT)/cargo-registry
CARGO_GIT_CACHE := $(CACHE_ROOT)/cargo-git
DIST_DIR := dist
RPM_SPEC := packaging/fedora/$(PACKAGE_NAME).spec

.PHONY: help build test smoke-test vendor dist srpm rpm install-local clean

help:
	@printf '%s\n' \
		'make build           Build release binary' \
		'make test            Run unit tests' \
		'make smoke-test      Run the app smoke test' \
		'make vendor          Vendor Rust crates for offline RPM builds' \
		'make dist            Generate source tarball for RPM builds' \
		'make srpm            Generate source RPM in dist/' \
		'make rpm             Generate binary RPM and source RPM in dist/' \
		'make install-local   Install local launcher and icons' \
		'make clean           Remove local build artifacts'

build:
	@set -euo pipefail; \
	if command -v cargo >/dev/null 2>&1 && command -v pkg-config >/dev/null 2>&1 && pkg-config --exists libcamera gtk4 libadwaita-1; then \
		cargo build --release --bin $(BIN); \
	else \
		mkdir -p "$(CARGO_REGISTRY_CACHE)" "$(CARGO_GIT_CACHE)"; \
		podman build -t "$(IMAGE_NAME)" -f Containerfile .; \
		podman run --rm \
			--userns=keep-id \
			--user "$$(id -u):$$(id -g)" \
			-v "$$(pwd):/workspace:Z" \
			-v "$(CARGO_REGISTRY_CACHE):/cargo/registry:Z" \
			-v "$(CARGO_GIT_CACHE):/cargo/git:Z" \
			-w /workspace \
			-e CARGO_HOME=/cargo \
			"$(IMAGE_NAME)" \
			/bin/bash --noprofile --norc -lc 'cargo build --manifest-path Cargo.toml --release --bin $(BIN)'; \
	fi

test:
	@set -euo pipefail; \
	if command -v cargo >/dev/null 2>&1 && command -v pkg-config >/dev/null 2>&1 && pkg-config --exists libcamera gtk4 libadwaita-1; then \
		cargo test --manifest-path Cargo.toml --lib --bin $(BIN); \
	else \
		mkdir -p "$(CARGO_REGISTRY_CACHE)" "$(CARGO_GIT_CACHE)"; \
		podman build -t "$(IMAGE_NAME)" -f Containerfile .; \
		podman run --rm \
			--userns=keep-id \
			--user "$$(id -u):$$(id -g)" \
			-v "$$(pwd):/workspace:Z" \
			-v "$(CARGO_REGISTRY_CACHE):/cargo/registry:Z" \
			-v "$(CARGO_GIT_CACHE):/cargo/git:Z" \
			-w /workspace \
			-e CARGO_HOME=/cargo \
			-e CARGO_TARGET_DIR=/tmp/galaxybook-target \
			"$(IMAGE_NAME)" \
			/bin/bash --noprofile --norc -lc 'cargo test --manifest-path Cargo.toml --lib --bin $(BIN)'; \
	fi

smoke-test: build
	./target/release/$(BIN) --smoke-test

vendor:
	@set -euo pipefail; \
	mkdir -p .cargo "$(CARGO_REGISTRY_CACHE)" "$(CARGO_GIT_CACHE)"; \
	podman build -t "$(IMAGE_NAME)" -f Containerfile .; \
	podman run --rm \
		--userns=keep-id \
		--user "$$(id -u):$$(id -g)" \
		-v "$$(pwd):/workspace:Z" \
		-v "$(CARGO_REGISTRY_CACHE):/cargo/registry:Z" \
		-v "$(CARGO_GIT_CACHE):/cargo/git:Z" \
		-w /workspace \
		-e CARGO_HOME=/cargo \
		"$(IMAGE_NAME)" \
		/bin/bash --noprofile --norc -lc 'cargo vendor vendor > .cargo/config.toml'

dist: vendor
	mkdir -p $(DIST_DIR)
	tar \
		--exclude='./.git' \
		--exclude='./target' \
		--exclude='./dist' \
		--transform='s,^\./,$(PACKAGE_NAME)-$(VERSION)/,' \
		-czf $(DIST_DIR)/$(PACKAGE_NAME)-$(VERSION).tar.gz \
		.

srpm: dist
	@set -euo pipefail; \
	mkdir -p "$(DIST_DIR)" "$(CARGO_REGISTRY_CACHE)" "$(CARGO_GIT_CACHE)"; \
	podman build -t "$(IMAGE_NAME)" -f Containerfile .; \
	podman run --rm \
		--userns=keep-id \
		--user "$$(id -u):$$(id -g)" \
		-v "$$(pwd):/workspace:Z" \
		-w /workspace \
		"$(IMAGE_NAME)" \
		/bin/bash --noprofile --norc -lc '\
			set -euo pipefail; \
			TOPDIR=/tmp/rpmbuild; \
			mkdir -p "$$TOPDIR"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}; \
			cp "$(DIST_DIR)/$(PACKAGE_NAME)-$(VERSION).tar.gz" "$$TOPDIR/SOURCES/"; \
			cp "$(RPM_SPEC)" "$$TOPDIR/SPECS/"; \
			rpmbuild -bs "$$TOPDIR/SPECS/$(PACKAGE_NAME).spec" --define "_topdir $$TOPDIR"; \
			cp "$$TOPDIR"/SRPMS/*.src.rpm /workspace/$(DIST_DIR)/; \
		'

rpm: dist
	@set -euo pipefail; \
	mkdir -p "$(DIST_DIR)" "$(CARGO_REGISTRY_CACHE)" "$(CARGO_GIT_CACHE)"; \
	podman build -t "$(IMAGE_NAME)" -f Containerfile .; \
	podman run --rm \
		--userns=keep-id \
		--user "$$(id -u):$$(id -g)" \
		-v "$$(pwd):/workspace:Z" \
		-w /workspace \
		"$(IMAGE_NAME)" \
		/bin/bash --noprofile --norc -lc '\
			set -euo pipefail; \
			TOPDIR=/tmp/rpmbuild; \
			mkdir -p "$$TOPDIR"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}; \
			cp "$(DIST_DIR)/$(PACKAGE_NAME)-$(VERSION).tar.gz" "$$TOPDIR/SOURCES/"; \
			cp "$(RPM_SPEC)" "$$TOPDIR/SPECS/"; \
			rpmbuild -ba "$$TOPDIR/SPECS/$(PACKAGE_NAME).spec" --define "_topdir $$TOPDIR"; \
			cp "$$TOPDIR"/SRPMS/*.src.rpm /workspace/$(DIST_DIR)/; \
			find "$$TOPDIR/RPMS" -type f -name "*.rpm" -exec cp {} /workspace/$(DIST_DIR)/ \; ; \
		'

install-local: build
	./scripts/install-galaxybook-camera-launcher.sh

clean:
	rm -rf target vendor .cargo dist
