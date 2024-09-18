use std::str::FromStr;

use bitcoin::absolute::LockTime;
use bitcoin::address::Address;
use bitcoin::{Amount, OutPoint, ScriptBuf, Sequence, TxIn, Witness};
use bitcoin::blockdata::transaction::{Transaction, TxOut};
use bitcoin::consensus::encode::serialize;
use bitcoin::transaction::Version;
use bitcoincore_rpc::{Auth, Client, RpcApi};


fn get_address() -> Address {
    Address::from_str("bcrt1q7czpufw20tn9qnnl97ht5lkl327fq5cec4tz4a").unwrap().assume_checked()
}

fn private_key() -> String {
    String::from("tprv8ZgxMBicQKsPfKvuWK3UN7q4wr7wTyfFakytnn8Wmg79ESLjrBqu9RbdfCpWiJDBAVBSSLeT1or6gPcmo46HKZf8GopAoSj5GxDvHrVvd11")
}

fn check_block_count(rpc_client: &Client) {
    let block_count = rpc_client.get_block_count().expect("Failed to get block count");
    println!("Current block count: {}", block_count);
}

fn check_balance(rpc_client: &Client) {
    let balance = rpc_client.get_balance(None, None).expect("Failed to get balance");
    println!("Current balance: {}", balance);
}

fn send_transaction(rpc_client: &Client) {
//     let s = Secp256k1::new();
//     let public_key = PublicKey::new(s.generate_keypair(&mut rand::thread_rng()).1);
//
// // Generate pay-to-pubkey-hash address.
//     let address = Address::p2pkh(&public_key, Network::Bitcoin);
    let to_address = get_address();

    // rpc_client.load_wallet("w1").unwrap();

    let utxos = rpc_client.list_unspent(None, None, None, None, None)
        .expect("Error listing unspent outputs");
    println!("{:?}", utxos[0]);
    let utxo = utxos[0].clone();

    // Create a TxOut (transaction output) with the amount to send and the scriptPubKey
    let tx_out = TxOut {
        value: Amount::from_btc(45.00).unwrap(), // Send 0.001 BTC
        script_pubkey: to_address.script_pubkey(),
    };

    let tx_in = TxIn {
        previous_output: OutPoint {
            txid: utxo.txid,
            vout: utxo.vout,
        },
        script_sig: ScriptBuf::default(),
        sequence: Sequence::default(),
        witness: Witness::new(),
    };

    // Create a new raw transaction with the output
    let raw_tx = Transaction {
        version: Version::ONE,
        lock_time: "0".parse().unwrap(),
        input: vec![tx_in],  // You will need to add inputs (UTXOs) here
        output: vec![tx_out],
    };

    // Serialize the raw transaction
    let serialized_tx = serialize(&raw_tx);

    // Convert the serialized transaction to hex
    let hex_tx = hex::encode(serialized_tx);

    // Send the raw transaction using the `sendrawtransaction` RPC call
    let txid = rpc_client.send_raw_transaction(hex_tx).expect("Error sending transaction");

    println!("Transaction sent! TxID: {:?}", txid);
}

fn generate_blocks(rpc_client: &Client) {
    let to_address = Address::from_str("bcrt1q9ukghgzjum37jdqzgrraup872echyxc8zsr8uk").unwrap();
    let checked_address = to_address.assume_checked();

    rpc_client.generate_to_address(500, &checked_address).unwrap();

    // rpc_client.generate_to_address(500, checked_address);
}

fn main() {
    // Set up RPC authentication and the bitcoind RPC URL
    let rpc_url = "http://127.0.0.1:18443"; // Local node running on port 8332
    let rpc_user = "abcd";
    let rpc_password = "abcd";

    // Initialize the bitcoind RPC client
    let rpc_client = Client::new(rpc_url, Auth::UserPass(rpc_user.to_string(), rpc_password.to_string()))
        .expect("Error creating RPC client");

    // rpc_client.load_wallet("w1").unwrap();

    check_block_count(&rpc_client);
    check_balance(&rpc_client);
    // generate_blocks(&rpc_client);
    send_transaction(&rpc_client);
}