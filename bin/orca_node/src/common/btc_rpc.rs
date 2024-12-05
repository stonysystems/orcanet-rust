use std::error::Error;
use std::fmt::{self, Display};
use std::str::FromStr;

use bitcoin::{Address, Amount, BlockHash, Txid};
use bitcoincore_rpc::json::ListTransactionResult;
use bitcoincore_rpc::{Auth, Client, Error as BtcError, RpcApi};
use serde::{Deserialize, Serialize};

use crate::impl_str_serde;

#[derive(Serialize, Deserialize, Debug)]
pub enum BTCNetwork {
    MainNet,
    TestNet,
    RegTest,
}

impl_str_serde!(BTCNetwork);

impl BTCNetwork {
    pub fn get_rpc_url(self) -> String {
        match self {
            BTCNetwork::MainNet => String::from("http://127.0.0.1:8332"),
            BTCNetwork::TestNet => String::from("http://127.0.0.1:18334"),
            BTCNetwork::RegTest => String::from("http://127.0.0.1:18443"),
        }
    }
}

pub struct RPCWrapper {
    rpc_client: Client,
}

impl RPCWrapper {
    pub fn new(network: BTCNetwork) -> Self {
        let rpc_url = network.get_rpc_url();
        let rpc_user = "user";
        let rpc_password = "password";

        return RPCWrapper {
            rpc_client: Client::new(
                rpc_url.as_str(),
                Auth::UserPass(rpc_user.to_string(), rpc_password.to_string()),
            )
            .expect("Error creating RPC client"),
        };
    }

    /// Send given amount (BTC) to address
    pub fn send_to_address(
        &self,
        address_string: &str,
        btc_amount: f64,
        comment: Option<&str>,
    ) -> Result<Txid, Box<dyn Error>> {
        let recipient_address = Address::from_str(address_string)?.assume_checked();
        let amount = Amount::from_btc(btc_amount).map_err(|e| e.to_string())?;

        self.rpc_client
            .send_to_address(
                &recipient_address,
                amount,
                comment,
                None,
                None,
                None,
                None,
                None,
            )
            .map_err(|e| Box::from(e))
    }

    /// Loads wallet. Returns true if load was successful or if the wallet is already loaded, false otherwise
    pub fn load_wallet(&self, wallet: &str) -> Result<(), BtcError> {
        match self.rpc_client.load_wallet(wallet) {
            Ok(v) => {
                tracing::info!("Loaded wallet {}", v.name);
                Ok(())
            }
            Err(err) => {
                if err.to_string().contains("already loaded") {
                    // It's fine if it's already loaded
                    tracing::info!("Wallet already loaded");
                    Ok(())
                } else {
                    tracing::error!("Failed to load wallet {:?}", err);
                    Err(err)
                }
            }
        }
    }

    pub fn check_block_count(&self) {
        let block_count = self
            .rpc_client
            .get_block_count()
            .expect("Failed to get block count");
        println!("Current block count: {}", block_count);
    }

    pub fn check_balance(&self) {
        let balance = self
            .rpc_client
            .get_balance(None, None)
            .expect("Failed to get balance");
        println!("Current balance: {}", balance);
    }

    /// Generate a single block with given address as the coinbase recipient
    pub fn generate_to_address(&self, address_string: &str) -> Result<BlockHash, Box<dyn Error>> {
        let recipient_address = Address::from_str(address_string)?.assume_checked();

        self.rpc_client
            .generate_to_address(1, &recipient_address)
            .map(|hashes| hashes[0])
            .map_err(|e| Box::from(e))
    }

    pub fn get_client(&self) -> &Client {
        &self.rpc_client
    }
}
