.PHONY: test build build-release int

test:
	cargo test

int:
	$(MAKE) build
	cd integration-tests && npm test

build:
	cargo near build non-reproducible-wasm --out-dir res

build-release:
	cargo near build reproducible-wasm --out-dir res
