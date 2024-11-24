## Prerequisites

1. Rust and cargo
2. SQLite 3
3. Bitcoin-core (modified)

## Setup

[//]: # (1. Download and set up our modified bitcoin core by following this)

[//]: # (   document: [a relative link]&#40;/docs/BitcoinCoreSetup.md&#41;.)

### Install Rust
Install rust by following [this](https://www.rust-lang.org/tools/install).

### Switch to nightly rust
```bash
rustup default nightly
```

### Install sqlite3
   - If you are on a Mac, it should already be installed. In case it's not, install with:
     ```bash
     brew install sqlite3
     ```
   - If you are on Linux, install with:
     ```bash
     sudo apt install sqlite3 
     ```

### Run the setup command
Use the `setup` command to set up the database and config file. Run this within the `orca_node` package root (i.e `orcanet-rust/orca_node`). 
   - Use the wallet name and BTC address that you generated in step 1. 
   - DB_PATH must be a path in an existing directory. The file will be created if not be present. 
   - APP_DATA_PATH can be any directory path within an existing directory. The directory will be created if not present.

```bash
cargo run --release setup --db-path DB_PATH \
 --app-data-path APP_DATA_PATH \
 --btc-address BTC_ADDRESS \
 --btc-wallet-name WALLET_NAME
```

## Run the node

[//]: # (TODO: Start bitcoin node at the start of start-node. Panic if bitcoind cannot be started.)

Use the `start-node` command to start the node.Run this within the `orca_node` package root (i.e `orcanet-rust/orca_node`).
```bash
cargo run --release start-node
```




