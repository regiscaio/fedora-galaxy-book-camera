SHELL := /usr/bin/env bash

APP_ID := com.caioregis.GalaxyBookCamera
BIN := galaxybook-camera
PACKAGE_NAME := galaxybook-camera
VERSION_SCRIPT := ./scripts/package-version.sh
VERSION := $(shell $(VERSION_SCRIPT))
IMAGE_NAME := localhost/galaxybook-camera-builder:fedora44
CACHE_ROOT ?= $(HOME)/.cache/galaxybook-camera-rust
CARGO_REGISTRY_CACHE := $(CACHE_ROOT)/cargo-registry
CARGO_GIT_CACHE := $(CACHE_ROOT)/cargo-git
DIST_DIR := dist
RPM_SPEC := packaging/fedora/$(PACKAGE_NAME).spec
RPM_VERSION_DEFINE := --define "pkg_version_override $(VERSION)"
PO_LANGS := en es it
POT_FILE := po/$(PACKAGE_NAME).pot
I18N_SOURCES := $(shell find src -name '*.rs' -print | sort)

.PHONY: help build test smoke-test vendor dist srpm rpm install-local i18n-extract i18n-update i18n-validate clean

help:
	@printf '%s\n' \
		'make build           Build release binary' \
		'make test            Run unit tests' \
		'make smoke-test      Run the app smoke test' \
		'make vendor          Vendor Rust crates for offline RPM builds' \
		'make i18n-extract    Generate gettext template in po/' \
		'make i18n-update     Merge current sources into po/*.po' \
		'make i18n-validate   Validate po/*.po with msgfmt' \
		'make dist            Generate source tarball for RPM builds' \
		'make srpm            Generate source RPM in dist/' \
		'make rpm             Generate binary RPM and source RPM in dist/' \
		'make install-local   Install local launcher and icons' \
		'make clean           Remove local build artifacts'

build:
	@set -euo pipefail; \
	if command -v cargo >/dev/null 2>&1 && command -v pkg-config >/dev/null 2>&1 && pkg-config --exists libcamera gtk4 libadwaita-1; then \
		APP_VERSION_OVERRIDE="$(VERSION)" cargo build --release --bin $(BIN); \
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
			-e APP_VERSION_OVERRIDE="$(VERSION)" \
			"$(IMAGE_NAME)" \
			/bin/bash --noprofile --norc -lc 'cargo build --manifest-path Cargo.toml --release --bin $(BIN)'; \
	fi

test:
	@set -euo pipefail; \
	if command -v cargo >/dev/null 2>&1 && command -v pkg-config >/dev/null 2>&1 && pkg-config --exists libcamera gtk4 libadwaita-1; then \
		APP_VERSION_OVERRIDE="$(VERSION)" cargo test --manifest-path Cargo.toml --lib --bin $(BIN); \
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
			-e APP_VERSION_OVERRIDE="$(VERSION)" \
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
			rpmbuild -bs "$$TOPDIR/SPECS/$(PACKAGE_NAME).spec" --define "_topdir $$TOPDIR" $(RPM_VERSION_DEFINE); \
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
			rpmbuild -ba "$$TOPDIR/SPECS/$(PACKAGE_NAME).spec" --define "_topdir $$TOPDIR" $(RPM_VERSION_DEFINE); \
			cp "$$TOPDIR"/SRPMS/*.src.rpm /workspace/$(DIST_DIR)/; \
			find "$$TOPDIR/RPMS" -type f -name "*.rpm" -exec cp {} /workspace/$(DIST_DIR)/ \; ; \
		'

install-local: build
	./scripts/install-galaxybook-camera-launcher.sh

i18n-extract:
	@set -euo pipefail; \
	mkdir -p po; \
	xgettext \
		--language=Rust \
		--from-code=UTF-8 \
		--package-name="$(PACKAGE_NAME)" \
		--package-version="$(VERSION)" \
		--keyword=tr \
		--keyword=trf \
		--keyword=trn:1,2 \
		-o "$(POT_FILE)" \
		$(I18N_SOURCES) \
		2> >(sed '/^vc-mtime:/d' >&2)

i18n-update: i18n-extract
	@set -euo pipefail; \
	for lang in $(PO_LANGS); do \
		msgmerge --update --backup=none "po/$$lang.po" "$(POT_FILE)" \
			2> >(sed '/^vc-mtime:/d' >&2); \
	done

i18n-validate:
	@set -euo pipefail; \
	for lang in $(PO_LANGS); do \
		msgfmt --check-format -o /dev/null "po/$$lang.po"; \
	done

clean:
	rm -rf target vendor .cargo dist
