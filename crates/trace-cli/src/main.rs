use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};

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
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => {
            let mut command = Cli::command();
            command.print_help()?;
            println!();
        }
        Some(command) => {
            println!("{command:?} is scaffolded and not implemented yet.");
        }
    }

    Ok(())
}
