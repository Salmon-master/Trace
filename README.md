# Trace

<img src="assets/trace-mark.png" alt="Trace logo" width="160">

Trace is a local hardware-context layer for embedded coding agents.

The project will turn KiCad schematics and component documentation into reliable,
queryable context for firmware development. KiCad connectivity and manufacturer
datasheets remain authoritative; generated summaries and search indexes are caches.

## Install Trace

The easiest way to install Trace is to download the latest platform bundle from
the [GitHub Releases page](https://github.com/Salmon-master/Trace/releases).

Windows:

1. Download the Windows ZIP and extract it.
2. Open PowerShell in the extracted directory.
3. Run:

```powershell
.\install\install.ps1
```

macOS or Linux:

1. Download the matching TAR archive and extract it.
2. Open a terminal in the extracted directory.
3. Run:

```bash
./install/install.sh
```

The installer detects Codex, Claude Code, and Cursor, then asks which agent
should receive the `trace-hardware` skill. It installs a prebuilt Trace binary,
so end users do not need Rust. KiCad is still required for schematic parsing.

For an existing checkout, the installer can list detected agents or target one
explicitly:

```powershell
.\install\install.ps1 -ListAgents
.\install\install.ps1 -Agent codex -Force
```

```bash
./install/install.sh --list-agents
./install/install.sh --agent claude --force
```

Use `--scope project` to install under the current project's `.codex`,
`.claude`, or `.cursor` directory. Use `--agent custom --skill-directory
PATH` for another Agent Skills-compatible tool.

The current release flow requires downloading and extracting the platform
bundle before running its installer. A one-line bootstrap installer and a
single-file Windows setup executable are planned, but are not available yet.

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

Read commands also support stable machine-readable output for agents:

```powershell
trace --format json overview
trace --format json component U1
trace --format json net "/PWM left"
trace --format json docs U1
trace --format json context U1
trace --format json pinout U1
trace --format json signal "/PWM left"
trace --format json sensor-read U1
```

JSON document results include the source URL, document type, cache paths,
resolver, match status, and parser metadata.

The high-level queries are intended for agents: `context` combines component
metadata and its KiCad pinout, `pinout` shows pins and connected nets, `signal`
shows the endpoints of a named signal, and `sensor-read` reports sensor-header
paths through passive components. `sensor-read` is a wiring report, not a
replacement for checking ADC registers and electrical limits in the datasheet.

The repository includes the `trace-hardware` agent skill under
`skill/trace-hardware`. Developers can build and install the CLI locally with:

```powershell
cargo install --path crates/trace-cli
```

## Publish a release

GitHub Releases are generated from version tags by
`.github/workflows/release.yml`. To publish a release:

```powershell
git add .
git commit -m "Prepare release"
git tag v0.1.0
git push origin main
git push origin v0.1.0
```

The workflow runs only for pushed tags beginning with `v`; an ordinary push to
`main` will not create a release. After pushing the tag, check the `Release`
workflow in the GitHub Actions tab. A release appears only after the publish
job completes successfully.

The workflow builds Windows, macOS, and Linux bundles, includes the skill and
platform installer, generates `checksums.txt`, and attaches the artifacts to
the GitHub Release. GitHub Releases are the initial source of truth; WinGet,
Homebrew, and Linux package manifests can be added later as wrappers around
these versioned artifacts.

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
