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
3. Use the `setup` command to set up the database and config file. Run this within the `orca_node` package root (i.e `orcanet-rust/orca_node`). 
   - Use the wallet name and BTC address that you generated in step 1. 
   - DB_PATH must be a path in an existing directory. The file will be created if not be present. 
   - APP_DATA_PATH can be any directory path within an existing directory. The directory will be created if not present.

   ```bash
   cargo run --release setup --db-path DB_PATH \
    --app-data-path APP_DATA_PATH \
    --btc-address BTC_ADDRESS \
    --btc-wallet-name WALLET_NAME
   ```

### Run the node
Use the `start-node` command to start the node.Run this within the `orca_node` package root (i.e `orcanet-rust/orca_node`).
```bash
cargo run --release start-node
```




