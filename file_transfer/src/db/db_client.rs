use clap::builder::Str;
use diesel::{Connection, ExpressionMethods, Insertable, Queryable, QueryDsl, QueryResult, RunQueryDsl, Selectable, update};
use diesel::dsl::{delete, insert_into};
use diesel::prelude::SqliteConnection;
use serde::Serialize;

use crate::common::{ConfigKey, OrcaNetConfig};
use crate::db::{DownloadedFileInfo, ProvidedFileInfo, table_schema};

fn create_connection(db_path: Option<String>) -> SqliteConnection {
    let _db_path = db_path
        .unwrap_or_else(|| OrcaNetConfig::get_str_from_config(ConfigKey::DBPath));

    match SqliteConnection::establish(_db_path.as_str()) {
        Ok(conn) => {
            tracing::info!("Opened connection");
            conn
        }
        Err(e) => {
            tracing::error!("Failed to open connection: {:?}", e);
            panic!("Can't proceed without DB connection");
        }
    }
}

pub struct ProvidedFilesTable {
    conn: SqliteConnection,
}

impl ProvidedFilesTable {
    pub fn new(db_path: Option<String>) -> Self {
        Self {
            conn: create_connection(db_path)
        }
    }

    pub fn insert_provided_file(&mut self, file_info: ProvidedFileInfo) -> QueryResult<usize> {
        use table_schema::provided_files::dsl::*;

        insert_into(provided_files)
            .values(&file_info)
            .execute(&mut self.conn)
    }

    pub fn remove_provided_file(&mut self, target_file_id: &str) -> QueryResult<usize> {
        use table_schema::provided_files::dsl::*;

        delete(provided_files.filter(file_id.eq(target_file_id)))
            .execute(&mut self.conn)
    }

    pub fn get_provided_file_info(&mut self, target_file_id: &str) -> QueryResult<ProvidedFileInfo> {
        use table_schema::provided_files::dsl::*;

        provided_files
            .filter(file_id.eq(target_file_id))
            .first::<ProvidedFileInfo>(&mut self.conn)
    }

    pub fn get_provided_files(&mut self) -> QueryResult<Vec<ProvidedFileInfo>> {
        use table_schema::provided_files::dsl::*;

        provided_files
            .load::<ProvidedFileInfo>(&mut self.conn)
    }

    pub fn increment_download_count(&mut self, target_file_id: &str) -> QueryResult<usize> {
        use table_schema::provided_files::dsl::*;

        update(provided_files.filter(file_id.eq(target_file_id)))
            .set(downloads_count.eq(downloads_count + 1))
            .execute(&mut self.conn)
    }

    pub fn set_provided_file_status(
        &mut self,
        target_file_id: &str,
        status_val: bool,
        timestamp_val: Option<i64>) -> QueryResult<usize> {
        use table_schema::provided_files::dsl::*;

        update(provided_files.filter(file_id.eq(target_file_id)))
            .set((
                status.eq(status_val as i32),
                provide_start_timestamp.eq(timestamp_val)
            ))
            .execute(&mut self.conn)
    }
}

pub struct DownloadedFilesTable {
    conn: SqliteConnection,
}

impl DownloadedFilesTable {
    pub fn new(db_path: Option<String>) -> Self {
        Self {
            conn: create_connection(db_path)
        }
    }

    pub fn insert_downloaded_file(&mut self, downloaded_file_info: DownloadedFileInfo) -> QueryResult<usize> {
        use table_schema::downloaded_files::dsl::*;

        insert_into(downloaded_files).values(&downloaded_file_info)
            .execute(&mut self.conn)
    }

    /// Get all downloads of a particular file_id
    pub fn get_downloaded_file_info(&mut self, target_file_id: &str) -> QueryResult<Vec<DownloadedFileInfo>> {
        use table_schema::downloaded_files::dsl::*;

        downloaded_files
            .filter(file_id.eq(&target_file_id))
            .load::<DownloadedFileInfo>(&mut self.conn)
    }

    pub fn get_downloaded_files(&mut self) -> QueryResult<Vec<DownloadedFileInfo>> {
        use table_schema::downloaded_files::dsl::*;

        downloaded_files
            .load::<DownloadedFileInfo>(&mut self.conn)
    }
}