# Trace

Trace is a local hardware-context layer for embedded coding agents.

The project will turn KiCad schematics and component documentation into reliable,
queryable context for firmware development. KiCad connectivity and manufacturer
datasheets remain authoritative; generated summaries and search indexes are caches.

## Current status

The first KiCad connectivity milestone is implemented. Trace can discover a KiCad
schematic, create `.hardware.toml`, export a KiCad XML netlist, parse components
and nets, and cache the resulting graph locally.

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

Run the CLI help:

```bash
cargo run -p trace-cli -- --help
```

## First hardware workflow

Run Trace from an embedded repository containing KiCad files:

```bash
trace init
trace overview
trace components
trace nets
trace component U1 --json
trace net LEFT_GO
trace trace LEFT_GO
```

Trace searches for `kicad-cli` on `PATH` and in common Windows KiCad install
locations. You can override discovery with `TRACE_KICAD_CLI`.

## Datasheet retrieval

When a component has an exact HTTP(S) datasheet URL in KiCad, Trace can retrieve
and convert it locally:

```bash
trace datasheet U1
```

The original PDF, converted Markdown, and provenance metadata are stored in the
machine-local Trace cache. Use `--refresh` to download and parse the source again:

```bash
trace datasheet U1 --refresh
```

Search the parsed Markdown with a Rust regular expression. The query is
case-sensitive unless the pattern includes `(?i)`:

```powershell
trace datasheet-search U1 --query "(?i)PA2|WO2|TCA0" --context 2
```

This reuses the cached datasheet. Add `--refresh` when the source PDF and
Markdown should be downloaded and generated again.

Retrieve verified related Microchip documents, such as the full family
datasheet, silicon errata, and AVR instruction-set manual:

```powershell
trace docs U1
trace docs-search U1 --query "(?i)ADC0|MUXPOS|CTRLA|RES" --context 2
```

Related documents are cached as separate PDFs and Markdown files with their
source URLs and document type recorded in metadata. Trace follows PDF links
from the primary Markdown only when they come from the same source host or an
official Microchip host.

The direct retrieval path uses an exact KiCad datasheet URL. If that URL is
absent, Trace can optionally resolve an exact MPN through Digi-Key when
credentials are configured:

```powershell
$env:TRACE_DIGIKEY_CLIENT_ID = "your-client-id"
$env:TRACE_DIGIKEY_CLIENT_SECRET = "your-client-secret"
$env:TRACE_DIGIKEY_SITE = "US"
$env:TRACE_DIGIKEY_LANGUAGE = "en"
$env:TRACE_DIGIKEY_CURRENCY = "USD"

trace datasheet U1
```

Trace only accepts a returned candidate when both the MPN and manufacturer match
the KiCad component. Ambiguous or non-exact matches are reported instead of
being silently selected. Digi-Key account/API credentials must remain outside
the repository.

## License

Trace is licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).
