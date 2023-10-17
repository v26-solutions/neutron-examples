use std::{env, path::PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand};
use cosmwasm_xtask::{network::Clean, Initialize, IntoForeground, NeutronLocalnet, StartLocal};
use xshell::{cmd, Shell};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "compile contracts for distribution")]
    Dist,
    #[command(
        about = "start a local neutron network instance (neutron + gaia + hermes + icq relayer)"
    )]
    StartLocal,
    #[command(about = "clean local network state, resetting the chains")]
    CleanLocalState,
    #[command(about = "clean local network artifacts including built binaries and source file")]
    CleanLocalAll,
    #[command(subcommand, about = "testing tasks")]
    Test(Test),
}

#[derive(Subcommand)]
enum Test {
    #[command(about = "start a local node then run e2e tests")]
    E2e { args: Option<String> },
}

pub fn main() -> Result<()> {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info");
    }

    env_logger::init();

    let cli = Cli::parse();

    let sh = Shell::new()?;

    let workspace_root =
        PathBuf::from(format!("{}/../", env!("CARGO_MANIFEST_DIR"))).canonicalize()?;

    sh.change_dir(workspace_root);

    match cli.command {
        Command::Dist => cosmwasm_xtask::ops::dist_workspace(&sh)?,
        Command::StartLocal => {
            NeutronLocalnet::initialize(&sh)?
                .start_local(&sh)?
                .into_foreground()?;
        }
        Command::CleanLocalState => NeutronLocalnet::clean_state(&sh)?,
        Command::CleanLocalAll => NeutronLocalnet::clean_all(&sh)?,
        Command::Test(cmd) => match cmd {
            Test::E2e { args } => {
                let _handle = NeutronLocalnet::initialize(&sh)?.start_local(&sh)?;
                cmd!(sh, "cargo t {args...} -- --nocapture --test-threads 1").run()?;
            }
        },
    }

    Ok(())
}
