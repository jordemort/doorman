use anyhow::Result;
use clap::{Args, Parser, ValueEnum};
//use nix::unistd;

pub mod config;
pub mod container;
pub mod door;
pub mod dos;
pub mod setuid;
pub mod user;
pub mod who;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
enum Commands {
    /// Launch a door
    Launch(LaunchArgs),

    /// Launch a door's configuration program
    Configure(SysopCmdArgs),

    /// Run a door's nighly maintenence
    Nightly(SysopCmdArgs),

    /// Show who's playing what
    Who(WhoArgs),
}
impl Commands {
    fn run(self) -> Result<()> {
        let config = config::Config::load(None)?;

        match self {
            Commands::Launch(args) => door::launch(&args, config),
            Commands::Configure(args) => door::configure(&args, &config),
            Commands::Nightly(args) => door::nightly(&args, &config),
            Commands::Who(args) => who::who_command(&args, &config),
        }
    }
}

#[derive(Args, Debug)]
pub struct LaunchArgs {
    door: String,

    #[arg(short, long, value_name = "USERNAME")]
    /// (SYSOP ONLY) User to run the door as
    user: Option<String>,

    #[arg(short = 'U', long, value_name = "UID")]
    /// (SYSOP ONLY) User ID to run the door as
    user_id: Option<u32>,

    #[arg(short, long, value_name = "\"Joan Q. Public\"")]
    /// (SYSOP ONLY) Display name of user to run the door as
    display_name: Option<String>,

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

#[derive(ValueEnum, Clone, Debug)]
#[value(rename_all = "lower")]
enum OutputFormat {
    JSON,
    YAML,
}

#[derive(Args, Debug)]
pub struct WhoArgs {
    /// (optional) Only show people playing DOOR
    door: Option<String>,

    #[arg(short, long)]
    /// Output format
    format: Option<OutputFormat>,
}

fn main() -> Result<()> {
    Commands::parse().run()
}
