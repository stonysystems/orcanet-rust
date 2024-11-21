use crate::common::{ConfigKey, OrcaNetConfig};
use crate::db::create_sqlite_connection;
use crate::SetupArgs;
use diesel::RunQueryDsl;
use rand::{thread_rng, Rng};
use rocket::serde::Deserialize;
use rocket::yansi::Paint;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

const DB_COMMANDS_FILE_PATH: &'static str = "src/assets/db_commands.yaml";
const DEFAULT_CONFIG_PATH: &'static str = "src/assets/default_config.json";
const BTC_CORE_SETUP_SCRIPT_PATH: &'static str = "src/assets/btc_core_setup.sh";

pub fn handle_setup(setup_args: &SetupArgs) {
    setup_database(setup_args.db_path.as_str());
    setup_btc_core();
    // setup_config_file(setup_args);
}

fn run_command(command: &str, comment: &str) {
    Command::new("sh")
        .arg("-c")
        .arg(command)
        .status()
        .expect(format!("{comment} command to run").as_str())
        .exit_ok()
        .expect(format!("{comment} command to succeed").as_str());
}

fn setup_btc_core() {
    let which_bitcoind = Command::new("sh")
        .arg("-c")
        .arg("which bitcoind")
        .output()
        .expect("which bitcoind to run");

    if which_bitcoind.status.success() {
        let bitcoind_loc = String::from_utf8(which_bitcoind.stdout)
            .expect("which bitcoind output to be utf8 string");
        println!(
            "bitcoind {} at {}. Setup skipped.",
            "found".green(),
            bitcoind_loc.trim()
        );
    } else {
        println!("{}", "bitcoind not found. Installing..".yellow());
        // Install only if bitcoind is not found
        Command::new("sh")
            .arg(BTC_CORE_SETUP_SCRIPT_PATH)
            .status()
            .expect("Btc core setup script to run")
            .exit_ok()
            .expect("Btc core setup script to succeed");
    }
}

#[derive(Debug, Deserialize)]
struct Queries {
    tables: HashMap<String, String>,
    indexes: HashMap<String, String>,
}

/// Create db file, tables and indexes
fn setup_database(db_path: &str) {
    println!("{}", "Setting up database...".yellow());

    let path = Path::new(db_path);
    if path.is_dir() || path.parent().is_some_and(|v| !v.exists()) {
        panic!("Db path must be valid file path in an existing directory");
    }

    let mut conn = create_sqlite_connection(Some(db_path.to_string()));
    let contents = fs::read_to_string(DB_COMMANDS_FILE_PATH)
        .expect("DB commands file path to be a valid file path that can be read");
    let queries: Queries =
        serde_yaml::from_str(&contents).expect("DB commands YAML to be a valid YAML");
    let mut failures = 0;

    for (table_name, query_string) in queries.tables {
        match diesel::sql_query(query_string.to_owned() + ";").execute(&mut conn) {
            Ok(_) => {
                println!("Table {table_name} {}", "created".green());
            }
            Err(e) => {
                failures += 1;
                println!(
                    "Table creation {} for {table_name}. Error {:?}",
                    "failed".red(),
                    e
                );
            }
        }
    }

    for (index_name, query_string) in queries.indexes {
        match diesel::sql_query(query_string.to_owned() + ";").execute(&mut conn) {
            Ok(_) => println!("Index {index_name} {}", "created".green()),
            Err(e) => {
                failures += 1;
                println!(
                    "Index creation {} for {index_name}. Error {:?}",
                    "failed".red(),
                    e
                );
            }
        }
    }

    if failures > 0 {
        println!(
            "{}",
            format!("Had {} failures in database setup.", failures.red()).yellow()
        );
    } else {
        println!("{}", "All steps in database setup completed!".green());
    }
}

fn setup_config_file(setup_args: &SetupArgs) {
    println!("{}", "\nSetting up config file...".yellow());

    // Copy default config to dest
    fs::copy(DEFAULT_CONFIG_PATH, OrcaNetConfig::get_config_file_path())
        .expect("Default config to be copied to config file path");

    // Update the config
    // TODO: Automate wallet creation and BTC address generation ?
    let seed = setup_args
        .seed
        .unwrap_or(thread_rng().gen_range(1..u64::MAX));

    let kv_pair = HashMap::from([
        (
            ConfigKey::DBPath.to_string(), //
            json!(setup_args.db_path),
        ),
        (
            ConfigKey::BTCWalletName.to_string(),
            json!(setup_args.btc_wallet_name),
        ),
        (
            ConfigKey::BTCAddress.to_string(),
            json!(setup_args.btc_address),
        ),
        (
            ConfigKey::SecretKeySeed.to_string(), //
            json!(seed),
        ),
    ]);

    OrcaNetConfig::modify_config_with_kv_pair(kv_pair).expect("DB path to be modified");

    println!("{}", "Config file setup completed!".green());
}
