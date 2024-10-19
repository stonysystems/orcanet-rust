use rusqlite::{Connection, params, Result as QueryResult, Row};

use crate::common::{ConfigKey, OrcaNetConfig};

pub struct DBClient {
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct FileInfo {
    pub file_id: String,
    pub file_path: String,
    pub file_name: String,
    pub downloads_count: usize,
}

impl FileInfo {
    pub fn from_row(row: &Row) -> QueryResult<Self> {
        Ok(Self {
            file_id: row.get::<_, String>(0)?,
            file_path: row.get::<_, String>(1)?,
            file_name: row.get::<_, String>(2)?,
            downloads_count: row.get::<_, usize>(3)?,
        })
    }
}

impl Clone for DBClient {
    fn clone(&self) -> Self {
        DBClient::new(Some(String::from(self.conn.path().unwrap())))
    }
}

impl DBClient {
    pub fn new(db_path: Option<String>) -> Self {
        let _db_path = db_path
            .unwrap_or_else(|| OrcaNetConfig::get_str_from_config(ConfigKey::DBPath));

        let conn = match Connection::open(_db_path) {
            Ok(conn) => {
                println!("Opened connection");
                conn
            }
            Err(e) => {
                eprintln!("Failed to open connection: {:?}", e);
                panic!("Can't proceed without DB connection");
            }
        };

        DBClient {
            conn
        }
    }

    pub fn get_tables(&self) -> QueryResult<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT name FROM sqlite_master WHERE type='table'")?;
        let table_names = stmt.query_map([], |row| row.get(0))?;

        Ok(table_names.collect::<QueryResult<_>>()?)
    }

    pub fn get_provided_files(&self) -> QueryResult<Vec<FileInfo>> {
        let mut stmt = self.conn.prepare("SELECT * FROM provided_files")?;
        let files = stmt.query_map([], FileInfo::from_row)?;

        Ok(files.collect::<QueryResult<_>>()?)
    }

    pub fn get_provided_file_info(&self, file_id: &str) -> QueryResult<FileInfo> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM provided_files WHERE file_id=?1")?;
        let file = stmt.query_row([file_id], FileInfo::from_row);

        return file;
    }

    pub fn insert_provided_file(&self, file_info: FileInfo) -> QueryResult<()> {
        self.conn.execute(
            "INSERT INTO provided_files VALUES (?1, ?2, ?3, ?4)",
            params![file_info.file_id, file_info.file_path, file_info.file_name, file_info.downloads_count],
        )?;

        Ok(())
    }

    pub fn remove_provided_file(&self, file_id: &str) -> QueryResult<()> {
        self.conn.execute(
            "DELETE FROM provided_files WHERE file_id = ?1",
            params![file_id],
        )?;

        Ok(())
    }

    pub fn increment_download_count(&self, file_id: &str) -> QueryResult<()> {
        self.conn.execute(
            "UPDATE provided_files SET downloads_count = downloads_count + 1 WHERE file_id = ?1",
            params![file_id],
        )?;

        Ok(())
    }
}