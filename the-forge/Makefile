VERSION := $(shell cat VERSION)
CARGO ?= cargo
export SOURCE_DATE_EPOCH ?= $(shell git log -1 --format=%ct 2>/dev/null || date +%s)

.PHONY: all build test check install clean dist repro verify

all: build

build:
	$(CARGO) build --locked --release --workspace

test:
	$(CARGO) test --locked --workspace

check:
	@if command -v cargo-clippy >/dev/null 2>&1 || cargo clippy --version >/dev/null 2>&1; then \
		$(CARGO) clippy --locked --workspace -- -D warnings; \
	else \
		echo "clippy not installed (dnf install clippy on RHEL)"; \
	fi
	@if command -v rustfmt >/dev/null 2>&1; then \
		$(CARGO) fmt --all -- --check; \
	else \
		echo "rustfmt not installed (dnf install rustfmt on RHEL)"; \
	fi

install:
	./scripts/install.sh $(DESTDIR)

clean:
	$(CARGO) clean

dist: build
	./scripts/build-release.sh

repro:
	./scripts/repro-build.sh

verify:
	./scripts/verify-release.sh

initramfs:
	./scripts/build-initramfs.sh

qemu: initramfs
	./scripts/run-qemu.sh

bootc-test:
	./scripts/run-bootc-test.sh

.PHONY: bootc-test
