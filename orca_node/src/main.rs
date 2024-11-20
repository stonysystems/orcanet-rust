#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use]
extern crate rocket;

use crate::cli_handlers::orca_node::start_orca_node;
use crate::cli_handlers::setup_handler::handle_setup;
use clap::{Parser, Subcommand};
use futures::{SinkExt, StreamExt};
use std::error::Error;
use std::str::FromStr;
use tokio::io::AsyncBufReadExt;

mod btc_rpc;
mod cli_handlers;
mod common;
mod db;
mod http;
mod macros;
mod network;
mod network_client;
mod request_handler;
mod utils;

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: OrcaCLICommand,
}

#[derive(Subcommand)]
enum OrcaCLICommand {
    Setup {
        #[arg(long, required = true)]
        db_path: String,
    },
    StartNode {
        #[arg(long, required = false)]
        seed: Option<u64>,
    },
}

#[rocket::main]
// #[tokio::main]
async fn main() {
    let args = Args::parse();

    match args.command {
        OrcaCLICommand::Setup { db_path } => {
            handle_setup(db_path);
        }
        OrcaCLICommand::StartNode { seed } => {
            // This will block
            // TODO: Daemonize later and add a stop node command to stop the daemon
            let _ = start_orca_node(seed).await;
        }
    }
}
