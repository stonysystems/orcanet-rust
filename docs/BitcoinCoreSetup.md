# Bitcoin core setup

## Setting up bitcoin-core

We’re using a modified version of bitcoin-core. So you will need to clone our git repo and build bitcoin-core from source.

**Warning:**

DO NOT use any standard installation methods (like homebrew or apt). You have to build from source. Our changes make it so that we have a modified mainnet and testnet that’s isolated from the real ones. If you use the standard bitcoin-core, your node will end up downloading the real mainnet or testnet blockchain.

Repo: https://github.com/Sethu98/bitcoin/tree/mod_27_v2

branch: mod_27_v2

NOTE: The following documentation assumes that you’re using a Unix based system, specifically MacOS or Ubuntu. If you’re using windows, we recommend using [WSL](https://learn.microsoft.com/en-us/windows/wsl/install) with Ubuntu.

### Step 1: Install dependencies

MacOS

```bash
brew install automake libtool boost pkg-config libevent miniupnpc
```

Ubuntu

```bash
sudo apt install -y build-essential libtool autotools-dev automake pkg-config bsdmainutils curl git libboost-all-dev sqlite3
```

### Step 2: Compile

Open a terminal and type the following commands:

```bash
git clone https://github.com/Sethu98/bitcoin.git
cd bitcoin
git checkout mod_27_v2
./autogen.sh
./configure
make -j16  # Tweak thread count based on your machine
```

### Step 3: Install

```bash
sudo make install
```

This should place the binary in the right directories

### Step 4: Configuration

Add the following to bitcoin configuration file:

```bash
server=1
rpcuser=user
rpcpassword=password

[regtest]
rpcallowip=127.0.0.1
rpcbind=127.0.0.1
bind=127.0.0.1
```

Configuration file would be:

- Mac - ~/Library/Application\ Support/Bitcoin/bitcoin.conf
- Ubuntu - ~/.bitcoin/bitcoin.conf

### Step 5: Test it out

After installing, we’re going to run some commands to check that it’s correctly installed by running in regtest mode. This mode is meant exclusively for testing privately so can use this mode to familiarize yourself with bitcoin-core and the RPCs.

Open a new terminal and run:

```bash
bitcoind -regtest

# Start in daemon mode
bitcoind -regtest -daemon
```

This should start the bitcoin node in regtest mode based on the configuration file. You should now be able to interact with it using bitcoin-cli.

Open another terminal and run these commands

```bash
bitcoin-cli createwallet alice
bitcoin-cli getnewaddress
# Say the response for getnewaddress is BITCOIN_ADDRESS

bitcoin-cli generatetoaddress 101 BITCOIN_ADDRESS
bitcoin-cli getbalance
# Should output 50.00
```

Explanation:

- We create a new wallet for you which stores all your keys and balances
- The  `getnewaddress` command generates a new bitcoin address. To be specific, it creates a new asymmetric key pair, stores them in the wallet, hashes the public key and outputs that as the bitcoin address
- The `generatetoaddress` command generates the given number of blocks with the given address as the recipient of the coinbase transaction for each of these blocks. Note that in the absence of any pending transactions, it will still mine blocks with just the coinbase transaction. Bitcoin allows having blocks with just the coinbase transaction. But in the real world, it won’t be a good choice to do that since you still have to do proof of work. So you might as well get transaction fees instead of mining with a single transaction.

### Step 5: Connect to our class’s blockchain network

Start the bitcoind in mainnet:

```bash
bitcoind -fallbackfee=0.0002
```

Now add our public node as a peer:

```bash
bitcoin-cli addnode "130.245.173.221:8333" add
```

This will connect to our public node and download our private blockchain. You can now start mining on this network. Try mining a block after creating a wallet with the following commands:

```bash
bitcoin-cli createwallet <wallet_name>
bitcoin-cli getnewaddress
# Say the response for getnewaddress is BITCOIN_ADDRESS

bitcoin-cli generatetoaddress 1 BITCOIN_ADDRESS
```

This might take anywhere from 5 to 20 minutes(or even more). It depends on your machine’s processing power and of course, luck. Save this address and use this for all transactions.

For connecting to testnet, start your node in testnet mode and connect to our testnet server:

```bash
bitcoind -testnet -fallbackfee=0.0002
bitcoin-cli -testnet addnode "130.245.173.221:18333" add
```