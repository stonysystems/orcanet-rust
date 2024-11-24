## Prerequisites

1. Rust and cargo
2. SQLite 3
3. Bitcoin-core (modified)

## Setup

[//]: # (1. Download and set up our modified bitcoin core by following this)

[//]: # (   document: [a relative link]&#40;/docs/BitcoinCoreSetup.md&#41;.)

### 1. Install sqlite3
- If you are on a Mac, it should already be installed. In case it's not, install with:
  ```bash
  brew install sqlite3
  ```
- If you are on Linux, install with:
  ```bash
  sudo apt install sqlite3 
  ```

### 2. Build the code
#### Install Rust
Install rust if not installed by following [this](https://www.rust-lang.org/tools/install).

#### Switch to nightly rust
```bash
rustup default nightly
```

#### Build the orca_node package
```bash
cd orca_node
cargo build --release
```

The binary for `orca_node` will be present in `target/release`. 
You need to use it directly from `repo_root/target/release` for the moment as we're using some relative paths in the `setup` command. Use it from `target/release` at least for the `setup` command in `Step 5`.
```bash
# Change dir to the location of orca_node binary
cd ../target/release
```

### 4. Setup bitcoin

#### Install
You can install in 2 ways:
- Install bitcoin-core manually by following [Bitcoin-core setup](/docs/BitcoinCoreSetup.md).
- Use the `setup-btc-core` command like this:
     ```bash
     ./orca_node setup-btc-core
     ```

#### Verify the installation
The installation step will install the bitcoind and bitcoin-cli binaries in the appropriate location. 
Follow [Step 5](/docs/BitcoinCoreSetup.md#step-5-test-it-out) from [Bitcoin-core setup](/docs/BitcoinCoreSetup.md) to test the installation.
  
#### Create wallet and BTC address
We need a wallet and a BTC address to start using the blockchain. We need a BTC address that we can use to receive BTC for coinbase transactions and from other users. The wallet manages our BTC addresses and their associated transactions.

Follow [Step 6](/docs/BitcoinCoreSetup.md#step-6-connect-to-our-classs-blockchain-network) from [Bitcoin-core setup](/docs/BitcoinCoreSetup.md) to create a new wallet and a new BTC address.
Note these down as we need it for the next step.

**Note**: We currently don't have user account and login features. If we add it in the future, then wallet and BTC address creation/configuration will be moved to the UI.

### 5. Run the setup command
Use the `setup` command to set up the database and config file. Run this within the `orca_node` package root (i.e `orcanet-rust/orca_node`).
   - Use the wallet name and BTC address that you generated in step 4. 
   - DB_PATH must be a path in an existing directory. The file will be created if not be present. 
   - APP_DATA_PATH can be any directory path within an existing directory. The directory will be created if not present.
   - SECRET_KEY_SEED should be some positive integer. It is **Optional** to provide this. It can be any positive integer (It SHOULD NOT be `0` as it is reserved for the relay server).

```bash
./orca_node setup --db-path DB_PATH \
 --app-data-path APP_DATA_PATH \
 --btc-address BTC_ADDRESS \
 --btc-wallet-name WALLET_NAME \
 --secret-key-seed SECRET_KEY_SEED # Optional: A random number will be used if not provided
```

You can use `-h` to get help about the `setup` command:
```bash
./orca_node setup -h
```

#### Verify the config file
The config file can be found in `~/.orcanet/config`. Verify that the file is present and has the values you gave for the `setup` command. It should be something like this:
```json
{
  "AppDataPath": "APP_DATA_PATH",
  "BTCAddress": "BTC_ADDRESS",
  "DBPath": "DB_PATH",
  "FileFeeRatePerKB": 0.0025,
  "ProxyFeeRatePerKB": 0.0025,
  "NetworkType": "MainNet",
  "ProxyConfig": null,
  "RunHTTPServer": true,
  "SecretKeySeed": 1234
}
```

You can modify any of these at any time.
### 6. Run the node

[//]: # (TODO: Start bitcoin node at the start of start-node. Panic if bitcoind cannot be started.)

Use the `start-node` command to start the node.Run this within the `orca_node` package root (i.e `orcanet-rust/orca_node`).
```bash
./orca_node start-node
```

## Doc links
1. [Backend design HLD](/docs/BackendHLD.md)
2. [Bitcoin core manual setup](/docs/BitcoinCoreSetup.md)


