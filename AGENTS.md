# Trace development guidance

Trace is a Rust workspace for exposing reliable hardware context to embedded
coding agents.

## Architectural rules

- Keep hardware logic in `trace-core`.
- Keep `trace-cli` as a thin command-line adapter.
- Treat KiCad as authoritative for electrical connectivity.
- Treat manufacturer datasheets as authoritative for component behaviour.
- Do not duplicate derived pin mappings in repository configuration.
- Preserve provenance for facts returned to agents.
- Keep generated data in disposable local caches, never in the repository.
- Do not add MCP-specific logic to the core library.

## Verification

Run these before handing off changes:

```bash
cargo fmt --check
cargo test
cargo clippy --workspace --all-targets --all-features -- -D warnings
```
