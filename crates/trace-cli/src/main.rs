use anyhow::{bail, Context, Result};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use std::path::{Path, PathBuf};

use trace_core::cache;
use trace_core::config::{HardwareConfig, KicadConfig, CONFIG_FILENAME};
use trace_core::datasheet::{
    retrieve_and_parse, retrieve_and_parse_candidate, retrieve_related_documents, search_document,
    DatasheetResolver, DigiKeyResolver,
};
use trace_core::kicad::{parse_kicad_xml, KicadCli};
use trace_core::project::{discover_kicad_files, find_config};
use trace_core::{component_pinout, sensor_inputs, signal, ConnectivityGraph};

#[derive(Debug, Parser)]
#[command(
    name = "trace",
    version,
    about = "Hardware context for embedded coding agents"
)]
struct Cli {
    /// Select human-readable or machine-readable output.
    #[arg(long, global = true, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Discover KiCad files and create .hardware.toml.
    Init,
    /// Show a concise hardware overview.
    Overview,
    /// List components in the hardware graph.
    Components,
    /// List named nets in the hardware graph.
    Nets,
    /// Show one component by reference.
    Component {
        reference: String,
        #[arg(long)]
        json: bool,
    },
    /// Show a component, its pinout, and connected nets.
    Context { reference: String },
    /// Show the KiCad pinout for one component.
    Pinout { reference: String },
    /// Show pins connected to a named net.
    Net { name: String },
    /// Show a named signal and its connected pins.
    Signal { name: String },
    /// Trace a named net through connected components.
    Trace { name: String },
    /// Show sensor-header inputs reaching a component through passive parts.
    SensorRead { reference: String },
    /// Retrieve and convert a component's exact datasheet URL.
    Datasheet {
        reference: String,
        #[arg(long)]
        refresh: bool,
    },
    /// Search a component's parsed datasheet Markdown with a regular expression.
    DatasheetSearch {
        reference: String,
        #[arg(long)]
        query: String,
        #[arg(long, default_value_t = 2)]
        context: usize,
        #[arg(long)]
        refresh: bool,
    },
    /// Retrieve the primary datasheet and verified related documents.
    Docs {
        reference: String,
        #[arg(long)]
        refresh: bool,
    },
    /// Search all cached documents for a component with a regular expression.
    DocsSearch {
        reference: String,
        #[arg(long)]
        query: String,
        #[arg(long, default_value_t = 2)]
        context: usize,
        #[arg(long)]
        refresh: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let json = matches!(cli.format, OutputFormat::Json);
    let current_directory =
        std::env::current_dir().context("could not determine current directory")?;

    match cli.command {
        None => {
            let mut command = Cli::command();
            command.print_help()?;
            println!();
        }
        Some(Commands::Init) => init(&current_directory, json)?,
        Some(Commands::Overview) => overview(&current_directory, json)?,
        Some(Commands::Components) => components(&current_directory, json)?,
        Some(Commands::Nets) => nets(&current_directory, json)?,
        Some(Commands::Component { reference, json }) => component(
            &current_directory,
            &reference,
            json || matches!(cli.format, OutputFormat::Json),
        )?,
        Some(Commands::Context { reference }) => context(&current_directory, &reference, json)?,
        Some(Commands::Pinout { reference }) => pinout(&current_directory, &reference, json)?,
        Some(Commands::Net { name }) => net(&current_directory, &name, false, json)?,
        Some(Commands::Signal { name }) => signal_command(&current_directory, &name, json)?,
        Some(Commands::Trace { name }) => net(&current_directory, &name, true, json)?,
        Some(Commands::SensorRead { reference }) => {
            sensor_read(&current_directory, &reference, json)?
        }
        Some(Commands::Datasheet { reference, refresh }) => {
            datasheet(&current_directory, &reference, refresh, json)?
        }
        Some(Commands::DatasheetSearch {
            reference,
            query,
            context,
            refresh,
        }) => datasheet_search(
            &current_directory,
            &reference,
            &query,
            context,
            refresh,
            json,
        )?,
        Some(Commands::Docs { reference, refresh }) => {
            docs(&current_directory, &reference, refresh, json)?
        }
        Some(Commands::DocsSearch {
            reference,
            query,
            context,
            refresh,
        }) => docs_search(
            &current_directory,
            &reference,
            &query,
            context,
            refresh,
            json,
        )?,
    }

    Ok(())
}

fn init(root: &Path, json: bool) -> Result<()> {
    let config_path = root.join(CONFIG_FILENAME);
    if config_path.exists() {
        bail!(
            "{} already exists; refusing to overwrite it",
            config_path.display()
        );
    }

    let discovered = discover_kicad_files(root).context("could not discover KiCad files")?;
    let config = HardwareConfig {
        schema: 1,
        kicad: KicadConfig {
            project: discovered
                .project
                .as_deref()
                .map(|path| relative_path(root, path)),
            schematic: relative_path(root, &discovered.schematic),
            pcb: discovered
                .pcb
                .as_deref()
                .map(|path| relative_path(root, path)),
        },
    };

    config
        .to_file(&config_path)
        .with_context(|| format!("could not create {}", config_path.display()))?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "schema": config.schema,
                "operation": "init",
                "config": config_path,
                "kicad": config.kicad,
            })
        );
        return Ok(());
    }

    println!("Created {}", config_path.display());
    println!("  schematic = {}", config.kicad.schematic);
    if let Some(project) = config.kicad.project {
        println!("  project   = {project}");
    }
    if let Some(pcb) = config.kicad.pcb {
        println!("  pcb       = {pcb}");
    }

    Ok(())
}

fn overview(root: &Path, json: bool) -> Result<()> {
    let (graph, cached) = load_graph(root)?;
    if json {
        println!(
            "{}",
            serde_json::json!({
                "schema": 1,
                "operation": "overview",
                "components": graph.components.len(),
                "pins": graph.pins.len(),
                "nets": graph.nets.len(),
                "graph": if cached { "cache-hit" } else { "generated" },
            })
        );
        return Ok(());
    }
    println!("Hardware Overview");
    println!("  components: {}", graph.components.len());
    println!("  pins:       {}", graph.pins.len());
    println!("  nets:       {}", graph.nets.len());
    println!(
        "  graph:      {}",
        if cached { "cache hit" } else { "generated" }
    );
    Ok(())
}

fn components(root: &Path, json: bool) -> Result<()> {
    let (graph, _) = load_graph(root)?;
    if json {
        println!(
            "{}",
            serde_json::json!({
                "schema": 1,
                "operation": "components",
                "components": graph.components,
            })
        );
        return Ok(());
    }
    for component in graph.components {
        println!(
            "{} — {}",
            component.reference,
            if component.value.is_empty() {
                "(no value)"
            } else {
                &component.value
            }
        );
    }
    Ok(())
}

fn nets(root: &Path, json: bool) -> Result<()> {
    let (graph, _) = load_graph(root)?;
    if json {
        let values = graph
            .nets
            .iter()
            .filter_map(|net| {
                net.name
                    .as_ref()
                    .map(|name| serde_json::json!({ "name": name, "pin_count": net.pins.len() }))
            })
            .collect::<Vec<_>>();
        println!(
            "{}",
            serde_json::json!({
                "schema": 1,
                "operation": "nets",
                "nets": values,
            })
        );
        return Ok(());
    }
    for net in graph.nets {
        if let Some(name) = net.name {
            println!("{name} ({} pins)", net.pins.len());
        }
    }

    Ok(())
}

fn component(root: &Path, reference: &str, json: bool) -> Result<()> {
    let (graph, _) = load_graph(root)?;
    let component = graph
        .components
        .iter()
        .find(|component| component.reference.eq_ignore_ascii_case(reference))
        .with_context(|| format!("component {reference} was not found"))?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "schema": 1,
                "operation": "component",
                "component": component,
            })
        );
    } else {
        println!("Reference: {}", component.reference);
        println!("Value:     {}", component.value);
        if let Some(footprint) = &component.footprint {
            println!("Footprint: {footprint}");
        }
        if let Some(manufacturer) = &component.manufacturer {
            println!("Maker:     {manufacturer}");
        }
        if let Some(mpn) = &component.mpn {
            println!("MPN:       {mpn}");
        }
        if let Some(datasheet) = &component.datasheet_url {
            println!("Datasheet: {datasheet}");
        }
    }

    Ok(())
}

fn context(root: &Path, reference: &str, json: bool) -> Result<()> {
    let (graph, _) = load_graph(root)?;
    let pinout = component_pinout(&graph, reference)?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "schema": 1,
                "operation": "context",
                "reference": pinout.component.reference.clone(),
                "component": pinout.component.clone(),
                "pins": pinout.pins,
                "datasheet": pinout.component.datasheet_url.clone(),
            })
        );
        return Ok(());
    }

    println!("Context: {}", pinout.component.reference);
    println!("  value: {}", pinout.component.value);
    if let Some(datasheet) = &pinout.component.datasheet_url {
        println!("  datasheet: {datasheet}");
    }
    println!("  pins: {}", pinout.pins.len());
    for pin in pinout.pins {
        let name = pin.name.as_deref().unwrap_or("unnamed");
        let nets = if pin.nets.is_empty() {
            "unconnected".to_string()
        } else {
            pin.nets.join(", ")
        };
        println!("    {} ({name}) -> {nets}", pin.number);
    }
    Ok(())
}

fn pinout(root: &Path, reference: &str, json: bool) -> Result<()> {
    let (graph, _) = load_graph(root)?;
    let pinout = component_pinout(&graph, reference)?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "schema": 1,
                "operation": "pinout",
                "reference": pinout.component.reference,
                "pins": pinout.pins,
            })
        );
        return Ok(());
    }

    println!("Pinout: {}", pinout.component.reference);
    for pin in pinout.pins {
        let name = pin.name.as_deref().unwrap_or("unnamed");
        let nets = if pin.nets.is_empty() {
            "unconnected".to_string()
        } else {
            pin.nets.join(", ")
        };
        println!("  {} ({name}) -> {nets}", pin.number);
    }
    Ok(())
}

fn signal_command(root: &Path, name: &str, json: bool) -> Result<()> {
    let (graph, _) = load_graph(root)?;
    let signal = signal(&graph, name)?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "schema": 1,
                "operation": "signal",
                "signal": signal,
            })
        );
        return Ok(());
    }

    println!("Signal: {}", signal.name);
    for pin in signal.pins {
        match pin.name {
            Some(name) => println!("  {}.{} ({name})", pin.reference, pin.number),
            None => println!("  {}.{}", pin.reference, pin.number),
        }
    }
    Ok(())
}

fn sensor_read(root: &Path, reference: &str, json: bool) -> Result<()> {
    let (graph, _) = load_graph(root)?;
    let inputs = sensor_inputs(&graph, reference)?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "schema": 1,
                "operation": "sensor-read",
                "target": reference,
                "inputs": inputs,
                "interpretation": "Wiring paths only; confirm ADC registers and electrical limits from the datasheet.",
            })
        );
        return Ok(());
    }

    println!("Sensor inputs: {reference}");
    if inputs.is_empty() {
        println!("  no sensor-header inputs found through series resistors");
        return Ok(());
    }
    for input in inputs {
        let adc = input
            .adc_channel
            .as_deref()
            .unwrap_or("ADC channel unknown");
        let series = input
            .series_component
            .map(|component| format!(" through {}", component.reference))
            .unwrap_or_default();
        println!(
            "  {}.{} ->{} {}.{} ({adc})",
            input.sensor.reference,
            input.sensor.number,
            series,
            input.target.reference,
            input.target.number,
        );
    }
    Ok(())
}

fn net(root: &Path, name: &str, traced: bool, json: bool) -> Result<()> {
    let (graph, _) = load_graph(root)?;
    let net = graph
        .nets
        .iter()
        .find(|net| net.name.as_deref() == Some(name))
        .with_context(|| format!("net {name} was not found"))?;

    let pins = net
        .pins
        .iter()
        .map(|pin_id| {
            let pin = graph
                .pins
                .get(*pin_id)
                .context("graph contained an invalid pin reference")?;
            let component = graph
                .components
                .get(pin.component)
                .context("graph contained an invalid component reference")?;
            Ok(serde_json::json!({
                "reference": component.reference,
                "number": pin.number,
                "name": pin.name,
            }))
        })
        .collect::<Result<Vec<_>>>()?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "schema": 1,
                "operation": if traced { "trace" } else { "net" },
                "name": name,
                "pins": pins,
            })
        );
        return Ok(());
    }

    if traced {
        println!("Trace: {name}");
    } else {
        println!("Net: {name}");
    }

    for pin_id in &net.pins {
        let pin = graph
            .pins
            .get(*pin_id)
            .context("graph contained an invalid pin reference")?;
        let component = graph
            .components
            .get(pin.component)
            .context("graph contained an invalid component reference")?;
        match &pin.name {
            Some(pin_name) => println!("  {}.{} ({})", component.reference, pin.number, pin_name),
            None => println!("  {}.{}", component.reference, pin.number),
        }
    }

    Ok(())
}

fn datasheet(root: &Path, reference: &str, refresh: bool, json: bool) -> Result<()> {
    let (graph, _) = load_graph(root)?;
    let component = graph
        .components
        .iter()
        .find(|component| component.reference.eq_ignore_ascii_case(reference))
        .with_context(|| format!("component {reference} was not found"))?;
    let document = if component.datasheet_url.is_some() {
        retrieve_and_parse(component, refresh)?
    } else {
        let resolver = DigiKeyResolver::from_env()
            .context("no KiCad datasheet URL; configure Digi-Key credentials to resolve by MPN")?;
        let candidates = resolver.resolve(component)?;
        let exact = candidates
            .iter()
            .filter(|candidate| candidate.exact_match && candidate.datasheet_url.is_some())
            .collect::<Vec<_>>();

        if exact.len() != 1 {
            if candidates.is_empty() {
                bail!("Digi-Key returned no product for component {reference}");
            }
            println!("Digi-Key returned candidates for {reference}:");
            for candidate in candidates {
                println!(
                    "  provider={} mpn={} manufacturer={} exact={} datasheet={}",
                    candidate.provider,
                    candidate.mpn.as_deref().unwrap_or("unknown"),
                    candidate.manufacturer.as_deref().unwrap_or("unknown"),
                    candidate.exact_match,
                    candidate.datasheet_url.as_deref().unwrap_or("none")
                );
            }
            bail!("no single exact datasheet match was safe to select");
        }

        retrieve_and_parse_candidate(component, exact[0], refresh)?
    };

    if json {
        println!(
            "{}",
            serde_json::json!({
                "schema": 1,
                "operation": "datasheet",
                "reference": component.reference,
                "document": document_json(&document),
            })
        );
        return Ok(());
    }

    println!("Datasheet: {}", component.reference);
    println!("  type:       {}", document.information.pdf_type);
    println!(
        "  confidence: {:.0}%",
        document.information.confidence * 100.0
    );
    println!("  pages:      {}", document.information.page_count);
    println!("  resolver:   {}", document.information.resolution_source);
    println!("  match:      {}", document.information.match_kind);
    println!("  source PDF: {}", document.source_pdf.display());
    println!("  Markdown:   {}", document.markdown.display());
    println!("  metadata:   {}", document.metadata.display());
    println!(
        "  cache:      {}",
        if document.cache_hit {
            "hit"
        } else {
            "generated"
        }
    );

    Ok(())
}

fn datasheet_search(
    root: &Path,
    reference: &str,
    query: &str,
    context: usize,
    refresh: bool,
    json: bool,
) -> Result<()> {
    let document = retrieve_datasheet(root, reference, refresh)?;
    let matches = search_document(&document, query, context)?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "schema": 1,
                "operation": "datasheet-search",
                "reference": reference,
                "pattern": query,
                "document": document_json(&document),
                "matches": matches,
            })
        );
        return Ok(());
    }

    println!("Datasheet search: {reference}");
    println!("  pattern: {query}");
    println!("  Markdown: {}", document.markdown.display());
    println!("  matches: {}", matches.len());

    for matched in matches {
        for (offset, line) in matched.context_before.iter().enumerate() {
            println!(
                "  {:>6}  {}",
                matched.line_number - matched.context_before.len() + offset,
                line
            );
        }
        println!("  {:>6}  {}", matched.line_number, matched.line);
        for (offset, line) in matched.context_after.iter().enumerate() {
            println!("  {:>6}  {}", matched.line_number + offset + 1, line);
        }
        println!();
    }

    Ok(())
}

fn docs(root: &Path, reference: &str, refresh: bool, json: bool) -> Result<()> {
    let (component, primary) = retrieve_component_datasheet(root, reference, refresh)?;
    let related = retrieve_related_documents(&component, &primary, refresh)?;

    if json {
        let related = related
            .into_iter()
            .map(|(candidate, document)| {
                serde_json::json!({
                    "kind": candidate.kind,
                    "url": candidate.url,
                    "discovery_source": candidate.discovery_source,
                    "document": document_json(&document),
                })
            })
            .collect::<Vec<_>>();
        println!(
            "{}",
            serde_json::json!({
                "schema": 1,
                "operation": "docs",
                "reference": reference,
                "primary": document_json(&primary),
                "related": related,
            })
        );
        return Ok(());
    }

    println!("Documents: {reference}");
    println!("  primary: {}", primary.markdown.display());
    println!("  related: {}", related.len());
    for (candidate, document) in related {
        println!("  - {}", candidate.kind);
        println!("    source: {}", candidate.url);
        println!("    Markdown: {}", document.markdown.display());
        println!(
            "    cache: {}",
            if document.cache_hit {
                "hit"
            } else {
                "generated"
            }
        );
    }

    Ok(())
}

fn docs_search(
    root: &Path,
    reference: &str,
    query: &str,
    context: usize,
    refresh: bool,
    json: bool,
) -> Result<()> {
    let (component, primary) = retrieve_component_datasheet(root, reference, refresh)?;
    let related = retrieve_related_documents(&component, &primary, refresh)?;
    let mut documents = vec![("primary-datasheet".to_string(), primary)];
    documents.extend(
        related
            .into_iter()
            .map(|(candidate, document)| (candidate.kind, document)),
    );

    if json {
        let results = documents
            .into_iter()
            .map(|(kind, document)| {
                let matches = search_document(&document, query, context)?;
                Ok(serde_json::json!({
                    "kind": kind,
                    "document": document_json(&document),
                    "matches": matches,
                }))
            })
            .collect::<Result<Vec<_>>>()?;
        println!(
            "{}",
            serde_json::json!({
                "schema": 1,
                "operation": "docs-search",
                "reference": reference,
                "pattern": query,
                "results": results,
            })
        );
        return Ok(());
    }

    println!("Document search: {reference}");
    println!("  pattern: {query}");
    for (kind, document) in documents {
        let matches = search_document(&document, query, context)?;
        println!("\n[{kind}] {} match(es)", matches.len());
        println!("  source: {}", document.information.source_url);
        for matched in matches {
            for (offset, line) in matched.context_before.iter().enumerate() {
                println!(
                    "  {:>6}  {}",
                    matched.line_number - matched.context_before.len() + offset,
                    line
                );
            }
            println!("  {:>6}  {}", matched.line_number, matched.line);
            for (offset, line) in matched.context_after.iter().enumerate() {
                println!("  {:>6}  {}", matched.line_number + offset + 1, line);
            }
            println!();
        }
    }

    Ok(())
}

fn document_json(document: &trace_core::datasheet::DatasheetDocument) -> serde_json::Value {
    serde_json::json!({
        "source_pdf": document.source_pdf,
        "markdown": document.markdown,
        "metadata": document.metadata,
        "information": document.information,
        "cache_hit": document.cache_hit,
    })
}

fn retrieve_datasheet(
    root: &Path,
    reference: &str,
    refresh: bool,
) -> Result<trace_core::datasheet::DatasheetDocument> {
    let (_, document) = retrieve_component_datasheet(root, reference, refresh)?;
    Ok(document)
}

fn retrieve_component_datasheet(
    root: &Path,
    reference: &str,
    refresh: bool,
) -> Result<(
    trace_core::graph::Component,
    trace_core::datasheet::DatasheetDocument,
)> {
    let (graph, _) = load_graph(root)?;
    let component = graph
        .components
        .iter()
        .find(|component| component.reference.eq_ignore_ascii_case(reference))
        .with_context(|| format!("component {reference} was not found"))?;

    if let Some(url) = component.datasheet_url.as_ref() {
        if url.is_empty() {
            bail!("component {reference} has an empty datasheet URL");
        }
        return Ok((component.clone(), retrieve_and_parse(component, refresh)?));
    }

    let resolver = DigiKeyResolver::from_env()
        .context("no KiCad datasheet URL; configure Digi-Key credentials to resolve by MPN")?;
    let candidates = resolver.resolve(component)?;
    let exact = candidates
        .iter()
        .filter(|candidate| candidate.exact_match && candidate.datasheet_url.is_some())
        .collect::<Vec<_>>();

    if exact.len() != 1 {
        if candidates.is_empty() {
            bail!("Digi-Key returned no product for component {reference}");
        }
        println!("Digi-Key returned candidates for {reference}:");
        for candidate in candidates {
            println!(
                "  provider={} mpn={} manufacturer={} exact={} datasheet={}",
                candidate.provider,
                candidate.mpn.as_deref().unwrap_or("unknown"),
                candidate.manufacturer.as_deref().unwrap_or("unknown"),
                candidate.exact_match,
                candidate.datasheet_url.as_deref().unwrap_or("none")
            );
        }
        bail!("no single exact datasheet match was safe to select");
    }

    Ok((
        component.clone(),
        retrieve_and_parse_candidate(component, exact[0], refresh)?,
    ))
}

fn load_graph(root: &Path) -> Result<(ConnectivityGraph, bool)> {
    let config_path =
        find_config(root).context("no .hardware.toml found in this directory or its parents")?;
    let config = HardwareConfig::from_file(&config_path)
        .with_context(|| format!("could not read {}", config_path.display()))?;
    let project_root = config_path
        .parent()
        .context("hardware configuration has no parent directory")?;
    let schematic = project_root.join(&config.kicad.schematic);
    if !schematic.is_file() {
        bail!(
            "configured schematic does not exist: {}",
            schematic.display()
        );
    }

    let graph_path = cache::graph_path(&schematic)?;
    if graph_path.is_file() {
        return Ok((cache::load_graph(&graph_path)?, true));
    }

    let kicad = KicadCli::discover()?;
    let netlist_path = graph_path
        .parent()
        .map(PathBuf::from)
        .context("graph cache path has no parent")?
        .join("netlist.kicadxml");
    kicad.export_netlist(&schematic, &netlist_path)?;
    let graph = parse_kicad_xml(&netlist_path)?;
    cache::store_graph(&graph_path, &graph)?;

    Ok((graph, false))
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
