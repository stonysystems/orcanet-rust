#![feature(proc_macro_hygiene, decl_macro)]
#![feature(exit_status_error)]
#[macro_use]
extern crate rocket;

use crate::cli_handlers::setup::{handle_setup, setup_btc_core};
use crate::cli_handlers::start_node::start_orca_node;
use clap::{Parser, Subcommand};
use futures::{SinkExt, StreamExt};
use std::error::Error;
use std::str::FromStr;
use tokio::io::AsyncBufReadExt;

mod cli_handlers;
mod common;
mod db;
mod http_server;
pub mod network;
mod proxy;
mod request_handler;

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: OrcaCLICommand,
}

#[derive(Parser)]
struct SetupArgs {
    #[arg(
        long,
        required = true,
        help = "Path of sqlite db file. Must be in an existing directory"
    )]
    db_path: String,
    #[arg(
        long,
        required = true,
        help = "Path where app data is stored included downloaded files when path is not explicitly specified. Must be in an existing directory"
    )]
    app_data_path: String,
    #[arg(long, required = true, help = "Name of the Bitcoin wallet")]
    btc_wallet_name: String,
    #[arg(
        long,
        required = true,
        help = "Bitcoin address within the given wallet"
    )]
    btc_address: String, // TODO: Can be removed if account creation is implemented later
    #[arg(long, required = false, help = "Seed for generating Peer ID")]
    secret_key_seed: Option<u64>,
}

#[derive(Subcommand)]
enum OrcaCLICommand {
    SetupBTCCore,
    SetupNode(SetupArgs),
    StartNode {
        #[arg(long, required = false)]
        seed: Option<u64>,
    },
}

// TODO: Create a new crate for CLI, move this there
#[tokio::main]
async fn main() {
    let args = Args::parse();

    match args.command {
        OrcaCLICommand::SetupBTCCore => {
            setup_btc_core();
        }
        OrcaCLICommand::SetupNode(setup_args) => {
            handle_setup(&setup_args);
        }
        OrcaCLICommand::StartNode { seed } => {
            // This will block
            // TODO: Daemonize later and add a stop node command to stop the daemon
            start_orca_node(seed).await.expect("Start node failed");
        }
    }
}
