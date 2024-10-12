use rusqlite::{Connection, params, Result as QueryResult};

use crate::common::OrcaNetConfig;

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

impl Clone for DBClient {
    fn clone(&self) -> Self {
        DBClient::new(Some(String::from(self.conn.path().unwrap())))
    }
}

impl DBClient {
    pub fn new(db_path: Option<String>) -> Self {
        let _db_path = db_path.unwrap_or(OrcaNetConfig::get_db_path());

        let conn = match Connection::open(_db_path) {
            Ok(conn) => {
                println!("Opened connection");
                conn
            }
            Err(e) => {
                eprintln!("Failed to open connection: {:?}", e);
                panic!("Oops");
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
        let files = stmt.query_map([], |row| {
            Ok(FileInfo {
                file_id: row.get::<_, String>(0)?,
                file_path: row.get::<_, String>(1)?,
                file_name: row.get::<_, String>(2)?,
                downloads_count: row.get::<_, usize>(3)?,
            })
        })?;

        Ok(files.collect::<QueryResult<_>>()?)
    }

    pub fn get_provided_file_info(&self, file_id: &str) -> QueryResult<FileInfo> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM provided_files WHERE file_id=?1")?;
        let file = stmt.query_row([file_id], |row| {
            Ok(FileInfo {
                file_id: row.get::<_, String>(0)?,
                file_path: row.get::<_, String>(1)?,
                file_name: row.get::<_, String>(2)?,
                downloads_count: row.get::<_, usize>(3)?,
            })
        });

        return file;
    }

    pub fn insert_provided_file(&self, file_info: FileInfo) -> QueryResult<()> {
        self.conn.execute(
            "INSERT INTO provided_files VALUES (?1, ?2, ?3, ?4)",
            params![file_info.file_id, file_info.file_path, file_info.file_name, file_info.downloads_count],
        )?;

        Ok(())
    }
}