# CargoMake by NeoSmart Technologies
# Written and maintained by Mahmoud Al-Qudsi <mqudsi@neosmart.net>
# Released under the MIT public license
# Obtain updates from https://github.com/neosmart/CargoMake

COLOR ?= auto # Valid COLOR options: {always, auto, never}
CARGO = cargo --color $(COLOR)

.PHONY: all bench build check clean doc install publish run test update

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

