## Prerequisites

1. Rust and cargo
2. SQLite 3
3. Bitcoin-core (modified)

## Setup

[//]: # (1. Download and set up our modified bitcoin core by following this)

[//]: # (   document: [a relative link]&#40;/docs/BitcoinCoreSetup.md&#41;.)

### 1. Build the code
In this step, we'll:
- Install `sqlite3` if not present
- Install `rust` if not present
- Build orca_node

Run the `orcanet-rust/build.sh` script (run as superuser if on linux):
```bash
sh ./build.sh
```

The binary will be present in `orcanet-rust/target/release`.

### 2. Setup bitcoin

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

### 3. Setup database and config file
Use the `setup-node` command to set up the database and config file that are needed for the node to work. For now, use the binary directly from `orcanet-rust/target/release` for the `setup-node` command as this command uses some relative paths. 
   - Use the wallet name and BTC address that you generated in step 2. 
   - DB_PATH and APP_DATA_PATH must be a path in an existing directory. The file/dir will be created if not be present.
   - SECRET_KEY_SEED should be some positive integer (It SHOULD NOT be `0` as it is reserved for the relay server). It is **Optional** to provide this.

```bash
./orca_node setup-node --db-path DB_PATH \
 --app-data-path APP_DATA_PATH \
 --btc-address BTC_ADDRESS \
 --btc-wallet-name WALLET_NAME \
 --secret-key-seed SECRET_KEY_SEED # Optional: A random number will be used if not provided
```

You can use `-h` to get help about the `setup-node` command:
```bash
./orca_node setup-node -h
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

You can modify these as per your needs.
### 4. Run the node

[//]: # (TODO: Start bitcoin node at the start of start-node. Panic if bitcoind cannot be started.)

Use the `start-node` command to start the node.
```bash
RUST_LOG=info ./orca_node start-node
```

To stop the node, type `exit` in the terminal where the node is running.

## Doc links
1. [Backend design HLD](/docs/BackendHLD.md)
2. [Bitcoin core manual setup](/docs/BitcoinCoreSetup.md)


