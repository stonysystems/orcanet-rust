use crate::db::create_sqlite_connection;
use diesel::RunQueryDsl;
use rocket::serde::Deserialize;
use std::collections::HashMap;
use std::fs;

const DB_COMMANDS_FILE_PATH: &'static str = "src/db/db_commands.yaml";

pub fn handle_setup(db_path: String) {
    // Create db file, tables and indexes
    handle_database_setup(db_path);

    // Set up config file
}

#[derive(Debug, Deserialize)]
struct QueriesModel {
    tables: HashMap<String, String>,
    indexes: HashMap<String, String>,
}

fn handle_database_setup(db_path: String) {
    let mut conn = create_sqlite_connection(Some(db_path.to_string()));
    let contents = fs::read_to_string(DB_COMMANDS_FILE_PATH)
        .expect("File read to work to proceed with table creation");
    let queries: QueriesModel = serde_yaml::from_str(&contents).expect("Failed to parse YAML");

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
