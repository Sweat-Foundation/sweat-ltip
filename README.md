# sweat-ltip

## Purpose

`sweat-ltip` is a NEAR smart contract that powers long-term incentive plan (LTIP) grants for Sweat Economy contributors. It manages vesting schedules, authorizes token payouts, supports clawbacks such as buybacks or terminations, and keeps spare treasury balances in sync with grant activity.

## Prerequisites

- `rustup` with the toolchain pinned to Rust `1.86.0` (automatically picked up from `rust-toolchain.toml`)
- `cargo` and standard Rust build tooling
- [`cargo-near`](https://github.com/near/cargo-near) for reproducible WASM builds and deployments
- Optional: [`near-cli`](https://near.cli.rs) if you plan to interact with a live NEAR network

Install the Rust toolchain and `cargo-near` if needed:

```bash
rustup target add wasm32-unknown-unknown
cargo install cargo-near
```

## Build

Compile the contract to a WASM artifact using the provided Makefile target (non-reproducible build):

```bash
make build
```

For a reproducible (release) build, run:

```bash
make build-release
```

Both targets delegate to `cargo near` and copy the resulting artifacts (WASM and ABI) into the `res/` directory.

## Test

Run the unit and integration test suites locally:

```bash
make test
```

The tests rely on `near-sdk`’s in-memory sandbox; no external NEAR node is required.
