use crate::common::{ConfigKey, OrcaNetConfig};
use crate::db::create_sqlite_connection;
use diesel::RunQueryDsl;
use rocket::serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::fs;

const DB_COMMANDS_FILE_PATH: &'static str = "src/assets/db_commands.yaml";
const DEFAULT_CONFIG_PATH: &'static str = "src/assets/default_config.json";

pub fn handle_setup(db_path: String, btc_address: String) {
    // Create db file, tables and indexes
    setup_database(db_path.as_str());

    // Set up config file
}

#[derive(Debug, Deserialize)]
struct Queries {
    tables: HashMap<String, String>,
    indexes: HashMap<String, String>,
}

fn setup_database(db_path: &str) {
    let mut conn = create_sqlite_connection(Some(db_path.to_string()));
    let contents = fs::read_to_string(DB_COMMANDS_FILE_PATH)
        .expect("File read to work to proceed with table creation");
    let queries: Queries =
        serde_yaml::from_str(&contents).expect("DB commands YAML to be a valid YAML");

    for (table_name, query_string) in queries.tables {
        match diesel::sql_query(query_string.to_owned() + ";").execute(&mut conn) {
            Ok(_) => println!("Table {table_name} created"),
            Err(e) => println!("Table creation failed for {table_name}. Error {:?}", e),
        }
    }

    for (index_name, query_string) in queries.indexes {
        match diesel::sql_query(query_string.to_owned() + ";").execute(&mut conn) {
            Ok(_) => println!("Index {index_name} created"),
            Err(e) => println!("Index creation failed for {index_name}. Error {:?}", e),
        }
    }
}

fn setup_config_file(db_path: &str, btc_address: &str) {
    // Copy default config to dest
    fs::copy(DEFAULT_CONFIG_PATH, OrcaNetConfig::get_config_file_path())
        .expect("Default config to be copied to config file path");

    // Update the config
    let kv_pair = HashMap::from([
        (ConfigKey::DBPath.to_string(), json!(db_path)),
        (ConfigKey::BTCAddress.to_string(), json!(btc_address)),
    ]);

    OrcaNetConfig::modify_config_with_kv_pair(kv_pair).expect("DB path to be modified");
}
