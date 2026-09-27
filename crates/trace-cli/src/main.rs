use anyhow::{bail, Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use std::path::{Path, PathBuf};

use trace_core::cache;
use trace_core::config::{HardwareConfig, KicadConfig, CONFIG_FILENAME};
use trace_core::datasheet::{
    retrieve_and_parse, retrieve_and_parse_candidate, retrieve_related_documents, search_document,
    DatasheetResolver, DigiKeyResolver,
};
use trace_core::kicad::{parse_kicad_xml, KicadCli};
use trace_core::project::{discover_kicad_files, find_config};
use trace_core::ConnectivityGraph;

#[derive(Debug, Parser)]
#[command(
    name = "trace",
    version,
    about = "Hardware context for embedded coding agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
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
    /// Show pins connected to a named net.
    Net { name: String },
    /// Trace a named net through connected components.
    Trace { name: String },
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
    let current_directory =
        std::env::current_dir().context("could not determine current directory")?;

    match cli.command {
        None => {
            let mut command = Cli::command();
            command.print_help()?;
            println!();
        }
        Some(Commands::Init) => init(&current_directory)?,
        Some(Commands::Overview) => overview(&current_directory)?,
        Some(Commands::Components) => components(&current_directory)?,
        Some(Commands::Nets) => nets(&current_directory)?,
        Some(Commands::Component { reference, json }) => {
            component(&current_directory, &reference, json)?
        }
        Some(Commands::Net { name }) => net(&current_directory, &name, false)?,
        Some(Commands::Trace { name }) => net(&current_directory, &name, true)?,
        Some(Commands::Datasheet { reference, refresh }) => {
            datasheet(&current_directory, &reference, refresh)?
        }
        Some(Commands::DatasheetSearch {
            reference,
            query,
            context,
            refresh,
        }) => datasheet_search(&current_directory, &reference, &query, context, refresh)?,
        Some(Commands::Docs { reference, refresh }) => {
            docs(&current_directory, &reference, refresh)?
        }
        Some(Commands::DocsSearch {
            reference,
            query,
            context,
            refresh,
        }) => docs_search(&current_directory, &reference, &query, context, refresh)?,
    }

    Ok(())
}

fn init(root: &Path) -> Result<()> {
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

fn overview(root: &Path) -> Result<()> {
    let (graph, cached) = load_graph(root)?;
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

fn components(root: &Path) -> Result<()> {
    let (graph, _) = load_graph(root)?;
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

fn nets(root: &Path) -> Result<()> {
    let (graph, _) = load_graph(root)?;
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
        println!("{}", serde_json::to_string_pretty(component)?);
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

fn net(root: &Path, name: &str, traced: bool) -> Result<()> {
    let (graph, _) = load_graph(root)?;
    let net = graph
        .nets
        .iter()
        .find(|net| net.name.as_deref() == Some(name))
        .with_context(|| format!("net {name} was not found"))?;

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

fn datasheet(root: &Path, reference: &str, refresh: bool) -> Result<()> {
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
) -> Result<()> {
    let document = retrieve_datasheet(root, reference, refresh)?;
    let matches = search_document(&document, query, context)?;

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

fn docs(root: &Path, reference: &str, refresh: bool) -> Result<()> {
    let (component, primary) = retrieve_component_datasheet(root, reference, refresh)?;
    let related = retrieve_related_documents(&component, &primary, refresh)?;

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
) -> Result<()> {
    let (component, primary) = retrieve_component_datasheet(root, reference, refresh)?;
    let related = retrieve_related_documents(&component, &primary, refresh)?;
    let mut documents = vec![("primary-datasheet".to_string(), primary)];
    documents.extend(
        related
            .into_iter()
            .map(|(candidate, document)| (candidate.kind, document)),
    );

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
