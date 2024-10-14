use std::str::FromStr;

use bitcoin::{Address, Amount};
use bitcoincore_rpc::{Auth, Client, RpcApi};

use crate::common::BTCNetwork;

pub struct RPCWrapper {
    rpc_client: Client,
}

impl RPCWrapper {
    pub fn new(network: BTCNetwork) -> Self {
        let rpc_url = network.get_rpc_url();
        let rpc_user = "user";
        let rpc_password = "password";

        return RPCWrapper {
            rpc_client: Client::new(rpc_url.as_str(), Auth::UserPass(rpc_user.to_string(), rpc_password.to_string()))
                .expect("Error creating RPC client")
        };
    }

    pub fn send_to_address(&self, address_string: &str, amount: Amount) {
        let recipient_address = match Address::from_str(address_string) {
            Ok(addr) => addr.assume_checked(),
            Err(e) => {
                eprintln!("Error parsing address {:?}", e);
                return;
            }
        };

        match self.rpc_client.send_to_address(&recipient_address, amount, None, None, None, None, None, None) {
            Ok(tx_id) => println!("TxID: {}", tx_id),
            Err(e) => println!("Failed to send amount to address {}. Error {:?}", address_string, e)
        }
    }

    pub fn load_wallet(&self, wallet: &str) {
        match self.rpc_client.load_wallet(wallet) {
            Ok(v) => println!("Loaded wallet {}", v.name),
            Err(e) => {
                println!("Failed to load wallet {:?}", e);
            }
        }
    }

    pub fn check_block_count(&self) {
        let block_count = self.rpc_client.get_block_count().expect("Failed to get block count");
        println!("Current block count: {}", block_count);
    }

    pub fn check_balance(&self) {
        let balance = self.rpc_client.get_balance(None, None).expect("Failed to get balance");
        println!("Current balance: {}", balance);
    }

    /// Generate a single block with given address as coinbase recipient
    pub fn generate_to_address(&self, address_string: &str) {
        let recipient_address = match Address::from_str(address_string) {
            Ok(addr) => addr.assume_checked(),
            Err(e) => {
                eprintln!("Error parsing address {:?}", e);
                return;
            }
        };

        let _ = self.rpc_client.generate_to_address(1, &recipient_address);
    }
}
