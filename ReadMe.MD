### Prerequisites
1. SQLite 3
2. Rust and cargo
3. Bitcoin-core (btcd support currently unavailable, can be added later)

### Setup steps
1. Download and set up our modified bitcoin core: [a relative link](/docs/BitcoinCoreSetup.md)
2. Create a sqlite3 database
```bash
sqlite3
.open <DB_FILE_PATH>
```
3. Add configuration file at `$HOME/.orcanet/config.json`
```bash
mkdir ~/.orcanet
vim ~/.orcanet/config.json
# Paste contents of sample_config.json and modify the values as necessary
# Make sure you give DBPath as the path of the file you created
```


