# CargoMake by NeoSmart Technologies
# Written and maintained by Mahmoud Al-Qudsi <mqudsi@neosmart.net>
# Released under the MIT public license
# Obtain updates from https://github.com/neosmart/CargoMake

COLOR ?= auto # Valid COLOR options: {always, auto, never}
CARGO = cargo --color $(COLOR)

.PHONY: all bench build check clean doc install publish run test update package

all: build

bench:
	@$(CARGO) bench

build:
	@$(CARGO) build

check:
	@$(CARGO) check

clean:
	@$(CARGO) clean

doc:
	@$(CARGO) doc

install: build
	@$(CARGO) install

publish:
	@$(CARGO) publish

run: build
	@$(CARGO) run

test: build
	@$(CARGO) test

update:
	@$(CARGO) update

target/x86_64-unknown-linux-musl/release/syngestures: src/*.rs Cargo.toml Cargo.lock
	env RUSTFLAGS= $(CARGO) build --release --target x86_64-unknown-linux-musl
	strip $@

syngestures.tar.gz: syngestures.toml target/release/syngestures README.md LICENSE target/x86_64-unknown-linux-musl/release/syngestures
	tar czf syngestures.tar.gz README.md LICENSE syngestures.toml -C target/x86_64-unknown-linux-musl/release/ syngestures

package: syngestures.tar.gz
