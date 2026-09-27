---
name: trace-hardware
description: Use the Trace CLI to inspect KiCad hardware connectivity, map signals and sensor inputs, and retrieve authoritative component documentation for embedded coding tasks.
---

# Trace Hardware

Use this skill when an embedded coding task depends on the board schematic, component pinout, signal wiring, sensor inputs, or datasheet register behavior.

## Operating rules

- Run Trace from the hardware project root, the directory containing `.hardware.toml`.
- Use the Trace CLI for hardware facts. Do not reconstruct connectivity from copied schematic text or guess from component reference designators.
- Treat KiCad connectivity as authoritative for wiring.
- Treat manufacturer datasheets and related documents as authoritative for electrical behavior and register semantics.
- Prefer `--format json` for inspection and subsequent reasoning. Every JSON response has `schema: 1` and an `operation` field.
- Preserve provenance in the answer: include the relevant KiCad-derived net/pin and datasheet source or cached document metadata.
- `sensor-read` reports wiring paths and ADC-channel hints only. Confirm ADC reference, enable, prescaler, conversion start, result-ready handling, and voltage limits with `datasheet-search` or `docs-search` before writing firmware.

## CLI workflow

Start with the board shape:

```powershell
trace --format json overview
trace --format json components
trace --format json nets
```

For a component or MCU:

```powershell
trace --format json context U1
trace --format json pinout U1
trace --format json component U1
```

For a signal:

```powershell
trace --format json signal "/PWM left"
trace --format json trace "/PWM left"
```

For sensor wiring:

```powershell
trace --format json sensor-read U1
```

For behavior and implementation details:

```powershell
trace --format json docs U1
trace --format json datasheet-search U1 --query "(?i)ADC0\\.MUXPOS|ADC0\\.COMMAND|ADC0\\.RES|REFSEL|PRESC|RESRDY" --context 2
trace --format json docs-search U1 --query "(?i)BODCFG|BODLEVEL|chip erase|UPDI" --context 2
```

Use `trace datasheet REF` when only the primary datasheet is needed. Use `trace docs REF` when related errata, family documentation, or instruction-set documentation may affect the answer.

## Installation and fallback

For a local checkout, build the CLI with:

```powershell
cargo install --path crates/trace-cli
```

If the `trace` binary is not on `PATH`, run the equivalent command from the Trace checkout:

```powershell
cargo run --manifest-path C:\DEV\Trace\Cargo.toml -p trace-cli -- --format json overview
```

If `.hardware.toml` is missing, initialize the project first with `trace init`. Do not overwrite an existing configuration; inspect it and ask before changing the selected schematic.
