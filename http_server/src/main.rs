#![feature(proc_macro_hygiene, decl_macro)]

mod btc_rpc;

#[macro_use]
extern crate rocket;

use std::string::String;
use bitcoincore_rpc::RpcApi;
use crate::btc_rpc::{BTCNetwork, RPCWrapper};


#[get("/")]
fn index() -> &'static str {
    "Hello, world!"
}

#[get("/blocks-count")]
fn get_block_count() -> String {
    let rpc_wrapper = RPCWrapper::new(BTCNetwork::MainNet);
    let client = rpc_wrapper.get_client();
    client.get_block_count().expect("Failed to get block count").to_string()
    // rpc_wrapper.block_count().to_string()
}

#[tokio::main]
async fn main() {
    rocket::ignite().mount("/", routes![index, get_block_count]).launch();
}