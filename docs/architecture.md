# Trace architecture

Trace has one authoritative core and multiple thin adapters.

```text
Coding agent
    │
    ├── trace CLI
    ├── future MCP adapter
    └── future editor integrations
            │
            ▼
        trace-core
            │
            ├── KiCad connectivity graph
            ├── local project cache
            └── future datasheet index
```

KiCad is authoritative for electrical connectivity. Manufacturer datasheets are
authoritative for component behaviour. Generated overviews and search indexes are
disposable derived data and must not replace either source.
