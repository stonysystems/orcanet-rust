use bitcoincore_rpc::{Auth, Client, RpcApi};
use bitcoin::{Address, Amount};
use std::str::FromStr;
use std::sync::{Arc, RwLock, RwLockReadGuard};
use tokio::task;

#[derive(Clone)]
pub enum BTCNetwork {
    MainNet,
    TestNet,
}

pub struct RPCWrapper {
    rpc_client: Arc<RwLock<Client>>,
    network: BTCNetwork,
}

impl BTCNetwork {
    pub fn get_rpc_url(self) -> String {
        match self {
            BTCNetwork::MainNet => String::from("http://127.0.0.1:8332"),
            BTCNetwork::TestNet => String::from("http://127.0.0.1:18334"),
        }
    }
}

impl RPCWrapper {
    pub fn new(network: BTCNetwork) -> Self {
        let rpc_url = network.clone().get_rpc_url();
        let rpc_user = "user";
        let rpc_password = "password";
        let rpc_client = Client::new(rpc_url.as_str(), Auth::UserPass(rpc_user.to_string(), rpc_password.to_string()))
            .expect("Error creating RPC client");

        return RPCWrapper {
            rpc_client: Arc::new(RwLock::new(rpc_client)),
            network: network.clone(),
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

        match self.rpc_client.read().unwrap()
            .send_to_address(&recipient_address, amount, None, None, None, None, None, None) {
            Ok(tx_id) => println!("TxID: {}", tx_id),
            Err(e) => println!("Failed to send amount to address {}. Error {:?}", address_string, e)
        }
    }

    pub fn load_wallet(&self, wallet: &str) {
        match self.rpc_client.read().unwrap().load_wallet(wallet) {
            Ok(v) => println!("Loaded wallet {}", v.name),
            Err(e) => {
                println!("Failed to load wallet {:?}", e);
            }
        }
    }

    pub fn get_client(&self) -> RwLockReadGuard<Client> {
        self.rpc_client.read().unwrap()
    }

    // pub fn mine_one(&self, to_address: &str, always: bool) {
    //     let do_mine = always || match self.rpc_client.read().unwrap().get_raw_mempool() {
    //         Ok(pending_txs) => pending_txs.len() > 0,
    //         Err(_) => false
    //     };
    //
    //     if !do_mine {
    //         return;
    //     }
    //
    //     let address = Address::from_str(to_address).unwrap().assume_checked();
    //     let rpc_client_arc = self.rpc_client.clone();
    //
    //     task::spawn(async move || {
    //         let rpc_client = rpc_client_arc.read().unwrap();
    //         rpc_client.generate_to_address(1, &address)
    //     });
    // }
}
