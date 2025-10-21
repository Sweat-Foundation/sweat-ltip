.PHONY: test build build-release

test:
	cargo test

build:
	cargo near build non-reproducible-wasm --out-dir res

build-release:
	cargo near build reproducible-wasm --out-dir res
