# Trace

Trace is a local hardware-context layer for embedded coding agents.

The project will turn KiCad schematics and component documentation into reliable,
queryable context for firmware development. KiCad connectivity and manufacturer
datasheets remain authoritative; generated summaries and search indexes are caches.

## Current status

The repository currently contains the initial Rust workspace scaffold. Hardware
graph construction, KiCad integration, and datasheet search will be implemented in
subsequent milestones.

## Workspace layout

```text
crates/trace-core    Reusable hardware-context library
crates/trace-cli     User-facing `trace` command
tests/fixtures       Small KiCad projects for integration tests
docs                 Architecture and design notes
```

## Development

Build the workspace:

```bash
cargo build
```

Run tests:

```bash
cargo test
```

## License

Trace is licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).
