### Prerequisites

1. SQLite 3
2. Rust and cargo
3. Bitcoin-core (modified)

### Setup

1. Download and set up our modified bitcoin core by following this
   document: [a relative link](/docs/BitcoinCoreSetup.md).
2. Switch to nightly rust

```bash
rustup default nightly
```
3. Build the orca_node code
```bash
cd orca_node
cargo build --release
```
3. Use the `setup` command to set up the database and config file. The binary will be in `<repo_root>/target/release`. Use the wallet name and BTC address that you generated in step 1. DB_PATH must be a path in an
   existing directory. The file need not be present.
```bash
cd ../target/release
./orca_node setup --db-path <DB_PATH> --btc-wallet-name <BTC_WALLET_NAME> --btc-address <BTC_ADDRESS>
```

### Run the node
Use the `start-node` command to start the node. The binary will be in `<repo_root>/target/release`.
```bash
./orca_node start-node
```




