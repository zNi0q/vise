# Thin wrapper over cargo, for people who expect `make install` to work.
PREFIX ?= $(HOME)/.local
BINDIR := $(PREFIX)/bin

.PHONY: all build test check install uninstall clean

all: build

build:
	cargo build --release

test:
	cargo test

# What CI runs, so a failure here is a failure there.
check:
	cargo fmt --all --check
	cargo clippy --all-targets -- -D warnings
	cargo test

install: build
	install -d $(BINDIR)
	install -m 755 target/release/vise $(BINDIR)/vise
	@echo "installed $(BINDIR)/vise"

uninstall:
	rm -f $(BINDIR)/vise

clean:
	cargo clean
