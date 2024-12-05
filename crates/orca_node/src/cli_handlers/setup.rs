use crate::common::config::{ConfigKey, OrcaNetConfig};
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

const ASSETS_PATH: &'static str = "../../crates/orca_node/src/assets";
const DB_COMMANDS_FILE_NAME: &'static str = "db_commands.yaml";
const DEFAULT_CONFIG_FILE_NAME: &'static str = "default_config.json";
const BTC_CORE_SETUP_SCRIPT_NAME: &'static str = "btc_core_setup.sh";

pub fn handle_setup(setup_args: &SetupArgs) {
    setup_database(setup_args.db_path.as_str());
    setup_config_file(setup_args);
}

#[derive(Debug, Deserialize)]
struct Queries {
    tables: HashMap<String, String>,
    indexes: HashMap<String, String>,
}

/// Create db file, tables and indexes
fn setup_database(db_path: &str) {
    println!("{}", "\nSetting up database...".yellow());

    let path = Path::new(db_path);
    if path.is_dir() || path.parent().is_some_and(|v| !v.exists()) {
        panic!("Db path must be valid file path in an existing directory");
    }

    let mut conn = create_sqlite_connection(Some(db_path.to_string()));
    let db_commands_file_path = Path::new(ASSETS_PATH).join(DB_COMMANDS_FILE_NAME);
    let contents = fs::read_to_string(db_commands_file_path)
        .expect("DB commands file path to be a valid file path that can be read");
    let queries: Queries =
        serde_yaml::from_str(&contents) //
            .expect("DB commands YAML to be a valid YAML that can be parsed");
    let mut failures = 0;

    // Create tables
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

    // Create indexes
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

/// Build bitcoin core from source if not present
pub fn setup_btc_core() {
    println!("{}", "\nSetting up bitcoin core...".yellow());

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
        let btc_setup_script_path = Path::new(ASSETS_PATH).join(BTC_CORE_SETUP_SCRIPT_NAME);

        Command::new("sh")
            .arg(btc_setup_script_path)
            .status()
            .expect("Btc core setup script to run")
            .exit_ok()
            .expect("Btc core setup script to succeed");
    }

    println!("{}", "Bitcoin core setup completed".green());
}

/// Create the config file in the expected location. Uses default values for most keys.
fn setup_config_file(setup_args: &SetupArgs) {
    println!("{}", "\nSetting up config file...".yellow());

    // Create app data path if it doesn't exist
    let app_data_path = Path::new(setup_args.app_data_path.as_str());

    if !app_data_path.exists() {
        // Not using create_dir_all because we don't want to create arbitrary number of folders somewhere
        fs::create_dir(app_data_path)
            .expect("App data path to be a valid path with an existing parent directory");
    }

    // Copy default config to dest
    let config_file_path = OrcaNetConfig::get_config_file_path();

    if let Some(parent) = config_file_path.parent() {
        if !parent.exists() {
            fs::create_dir(parent)
                .expect("Config file path to be in a parent under an existing directory");
        }
    }

    let default_config_file_path = Path::new(ASSETS_PATH).join(DEFAULT_CONFIG_FILE_NAME);
    fs::copy(default_config_file_path, &config_file_path)
        .expect("Default config to be copied to config file path");

    // Update the config
    // TODO: Automate wallet creation and BTC address generation ?
    let seed = setup_args
        .secret_key_seed
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
        (
            ConfigKey::AppDataPath.to_string(), //
            json!(app_data_path),
        ),
    ]);

    OrcaNetConfig::modify_config_with_kv_pair(kv_pair) //
        .expect("Config file update failed");

    println!("{}", "Config file setup completed!".green());
}
