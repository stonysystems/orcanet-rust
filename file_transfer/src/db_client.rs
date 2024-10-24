use diesel::{Connection, ExpressionMethods, Insertable, Queryable, QueryDsl, QueryResult, RunQueryDsl, Selectable, update};
use diesel::dsl::{delete, insert_into};
use diesel::prelude::SqliteConnection;
use serde::Serialize;

use schema::{downloaded_files, provided_files};

use crate::common::{ConfigKey, OrcaNetConfig};
use crate::db_client::schema::provided_files::provide_start_timestamp;

pub struct DBClient {
    conn: SqliteConnection,
    db_path: String,
}

impl Clone for DBClient {
    fn clone(&self) -> Self {
        DBClient::new(Some(self.db_path.clone()))
    }
}

impl DBClient {
    pub fn new(db_path: Option<String>) -> Self {
        let _db_path = db_path
            .unwrap_or_else(|| OrcaNetConfig::get_str_from_config(ConfigKey::DBPath));

        let conn = match SqliteConnection::establish(_db_path.as_str()) {
            Ok(conn) => {
                tracing::info!("Opened connection");
                conn
            }
            Err(e) => {
                tracing::error!("Failed to open connection: {:?}", e);
                panic!("Can't proceed without DB connection");
            }
        };

        DBClient {
            conn,
            db_path: _db_path,
        }
    }

    //
    // pub fn get_tables(&self) -> QueryResult<Vec<String>> {
    //     let mut stmt = self.conn.prepare("SELECT name FROM sqlite_master WHERE type='table'")?;
    //     let table_names = stmt.query_map([], |row| row.get(0))?;
    //
    //     Ok(table_names.collect::<QueryResult<_>>()?)
    // }

    pub fn insert_provided_file(&mut self, file_info: ProvidedFileInfo) -> QueryResult<usize> {
        use schema::provided_files::dsl::*;

        insert_into(provided_files)
            .values(&file_info)
            .execute(&mut self.conn)
    }

    pub fn remove_provided_file(&mut self, target_file_id: &str) -> QueryResult<usize> {
        use schema::provided_files::dsl::*;

        delete(provided_files.filter(file_id.eq(target_file_id)))
            .execute(&mut self.conn)
    }

    pub fn get_provided_file_info(&mut self, target_file_id: &str) -> QueryResult<ProvidedFileInfo> {
        use schema::provided_files::dsl::*;

        provided_files
            .filter(file_id.eq(target_file_id))
            .first::<ProvidedFileInfo>(&mut self.conn)
    }

    pub fn get_provided_files(&mut self) -> QueryResult<Vec<ProvidedFileInfo>> {
        use schema::provided_files::dsl::*;

        provided_files
            .load::<ProvidedFileInfo>(&mut self.conn)
    }

    pub fn increment_download_count(&mut self, target_file_id: &str) -> QueryResult<usize> {
        use schema::provided_files::dsl::*;

        update(provided_files.filter(file_id.eq(target_file_id)))
            .set(downloads_count.eq(downloads_count + 1))
            .execute(&mut self.conn)
    }

    pub fn set_provided_file_status(
        &mut self,
        target_file_id: &str,
        status_val: bool,
        timestamp_val: Option<i64>) -> QueryResult<usize> {
        use schema::provided_files::dsl::*;

        update(provided_files.filter(file_id.eq(target_file_id)))
            .set((
                status.eq(status_val as i32),
                provide_start_timestamp.eq(timestamp_val)
            ))
            .execute(&mut self.conn)
    }

    pub fn insert_downloaded_file(&mut self, downloaded_file_info: DownloadedFileInfo) -> QueryResult<usize> {
        use schema::downloaded_files::dsl::*;

        insert_into(downloaded_files).values(&downloaded_file_info)
            .execute(&mut self.conn)
    }

    /// Get all downloads of a particular file_id
    pub fn get_downloaded_file_info(&mut self, target_file_id: &str) -> QueryResult<Vec<DownloadedFileInfo>> {
        use schema::downloaded_files::dsl::*;

        downloaded_files
            .filter(file_id.eq(&target_file_id))
            .load::<DownloadedFileInfo>(&mut self.conn)
    }

    pub fn get_downloaded_files(&mut self) -> QueryResult<Vec<DownloadedFileInfo>> {
        use schema::downloaded_files::dsl::*;

        downloaded_files
            .load::<DownloadedFileInfo>(&mut self.conn)
    }
}

mod schema {
    diesel::table! {
        provided_files (file_id) {
            file_id -> Text,
            file_path -> Text,
            file_name -> Text,
            downloads_count -> Integer,
            status -> Integer,
            provide_start_timestamp -> Nullable<BigInt>
        }
    }

    diesel::table! {
        downloaded_files {
            id -> Text,
            file_id -> Text,
            file_path -> Text,
            file_name -> Text,
            file_size_kb -> Float,
            fee_rate_per_kb -> Nullable<Float>,
            price -> Nullable<Float>,
            payment_tx_id -> Nullable<Text>,
            peer_id -> Text,
            download_timestamp -> BigInt
        }
    }
}

#[derive(Debug, Clone, Serialize, Insertable, Queryable, Selectable)]
#[diesel(table_name = provided_files)]
pub struct ProvidedFileInfo {
    pub file_id: String,
    pub file_path: String,
    pub file_name: String,
    pub downloads_count: i32,
    pub status: i32,
    pub provide_start_timestamp: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Insertable, Queryable, Selectable)]
#[diesel(table_name = downloaded_files)]
pub struct DownloadedFileInfo {
    pub id: String,
    pub file_id: String,
    pub file_path: String,
    pub file_name: String,
    pub file_size_kb: f32,
    pub fee_rate_per_kb: Option<f32>, // May not be rate but fixed price
    pub price: Option<f32>, // Size * rate if rate is present
    pub payment_tx_id: Option<String>, // Transaction may not have started, so can be NULL ?
    pub peer_id: String,
    pub download_timestamp: i64,
}