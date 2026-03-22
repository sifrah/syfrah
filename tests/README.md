# E2E Tests

End-to-end tests that verify cross-layer scenarios on a single machine.

## Prerequisites

- Linux (WireGuard kernel module)
- Root access (interface creation)
- Rust toolchain

## Running

```bash
# Run all E2E tests
sudo cargo test --test '*' -- --ignored

# Run a specific scenario
sudo cargo test --test fabric_mesh -- --ignored
```

## Writing tests

See [handbook/testing.md](../handbook/testing.md) for the full testing strategy.

Each file in this directory is a test scenario. Use `tests/helpers/` for shared utilities.
