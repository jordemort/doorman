use clap::{Args, Parser, Subcommand};

pub mod cfg;
pub mod door;
pub mod dos;

//pub mod context;
//pub mod doorsys;
//pub mod write_dos;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Launch a door
    Launch(LaunchArgs),

    /// Launch a door's configuration program
    Configure(SysopCmdArgs),

    /// Run a door's nighly maintenence
    Nightly(SysopCmdArgs),
}

#[derive(Args, Debug)]
pub struct LaunchArgs {
    door: String,

    #[arg(short, long, value_name = "USERNAME")]
    /// User to run the door as
    user: Option<String>,

    #[arg(short, long)]
    /// Don't translate from ANSI+CP437
    raw: bool,
}

#[derive(Args, Debug)]
pub struct SysopCmdArgs {
    door: String,

    #[arg(short, long)]
    /// Fail immediate if door is busy
    nowait: bool,
}

fn main() {
    let config = match cfg::Config::load() {
        Ok(config) => config,
        Err(e) => {
            eprintln!("{0}", e);
            std::process::exit(1);
        }
    };

    let cli = Cli::parse();

    match &cli.command {
        Commands::Launch(args) => door::launch(&args, &config),
        Commands::Configure(args) => door::configure(&args, &config),
        Commands::Nightly(args) => door::nightly(&args, &config),
    };
}
