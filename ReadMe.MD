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

3. Use the `setup` command to set up the database and config file. Use the wallet name and BTC address that you generated in step 1. DB_PATH must be a path in an
   existing directory. The file need not be present.
```bash
cargo run setup --db-path <DB_PATH> --btc-wallet-name <BTC_WALLET_NAME> --btc-address <BTC_ADDRESS>
```

### Run the node
Use the `start-node` command to start the node
```bash
cargo run start-node
```




