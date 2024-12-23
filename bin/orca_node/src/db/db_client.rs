use clap::builder::Str;
use diesel::dsl::{delete, insert_into};
use diesel::prelude::SqliteConnection;
use diesel::{
    update, Connection, ExpressionMethods, Insertable, QueryDsl, QueryResult, Queryable,
    RunQueryDsl, Selectable,
};
use serde::Serialize;

use crate::common::config::{ConfigKey, OrcaNetConfig};
use crate::db::{
    table_schema, DownloadedFileInfo, PaymentInfo, ProvidedFileInfo, ProxyClientInfo,
    ProxySessionInfo,
};

pub fn create_sqlite_connection(db_path: Option<String>) -> SqliteConnection {
    let _db_path = db_path.unwrap_or_else(|| OrcaNetConfig::get_str_from_config(ConfigKey::DBPath));

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

// Proc macro is probably cleaner but more work. Do later if required.
macro_rules! fn_table_new {
    () => {
        pub fn new(db_path: Option<String>) -> Self {
            Self {
                conn: create_sqlite_connection(db_path),
            }
        }
    };
}

// TODO: make all methods static and pass &mut SqliteConnection as param instead
pub struct ProvidedFilesTable {
    conn: SqliteConnection,
}

impl ProvidedFilesTable {
    fn_table_new!();

    pub fn insert_provided_file(&mut self, file_info: &ProvidedFileInfo) -> QueryResult<usize> {
        use table_schema::provided_files::dsl::*;

        insert_into(provided_files)
            .values(file_info)
            .execute(&mut self.conn)
    }

    pub fn remove_provided_file(&mut self, target_file_id: &str) -> QueryResult<usize> {
        use table_schema::provided_files::dsl::*;

        delete(provided_files.find(target_file_id)).execute(&mut self.conn)
    }

    pub fn get_provided_file_info(
        &mut self,
        target_file_id: &str,
    ) -> QueryResult<ProvidedFileInfo> {
        use table_schema::provided_files::dsl::*;

        provided_files
            .find(target_file_id)
            .first::<ProvidedFileInfo>(&mut self.conn)
    }

    pub fn get_provided_files(&mut self) -> QueryResult<Vec<ProvidedFileInfo>> {
        use table_schema::provided_files::dsl::*;

        provided_files.load::<ProvidedFileInfo>(&mut self.conn)
    }

    pub fn increment_download_count(&mut self, target_file_id: &str) -> QueryResult<usize> {
        use table_schema::provided_files::dsl::*;

        update(provided_files.find(target_file_id))
            .set(downloads_count.eq(downloads_count + 1))
            .execute(&mut self.conn)
    }

    pub fn set_provided_file_status(
        &mut self,
        target_file_id: &str,
        status_val: bool,
        timestamp_val: Option<i64>,
    ) -> QueryResult<usize> {
        use table_schema::provided_files::dsl::*;

        update(provided_files.find(target_file_id))
            .set((
                status.eq(status_val as i32),
                provide_start_timestamp.eq(timestamp_val),
            ))
            .execute(&mut self.conn)
    }
}

pub struct DownloadedFilesTable {
    conn: SqliteConnection,
}

impl DownloadedFilesTable {
    fn_table_new!();

    pub fn insert_downloaded_file(
        &mut self,
        downloaded_file_info: &DownloadedFileInfo,
    ) -> QueryResult<usize> {
        use table_schema::downloaded_files::dsl::*;

        insert_into(downloaded_files)
            .values(downloaded_file_info)
            .execute(&mut self.conn)
    }

    /// Get all downloads of a particular file_id
    pub fn get_downloaded_file_info(
        &mut self,
        target_file_id: &str,
    ) -> QueryResult<Vec<DownloadedFileInfo>> {
        use table_schema::downloaded_files::dsl::*;

        downloaded_files
            .filter(file_id.eq(&target_file_id))
            .load::<DownloadedFileInfo>(&mut self.conn)
    }

    pub fn get_downloaded_files(&mut self) -> QueryResult<Vec<DownloadedFileInfo>> {
        use table_schema::downloaded_files::dsl::*;

        downloaded_files.load::<DownloadedFileInfo>(&mut self.conn)
    }
}

pub struct ProxyClientsTable {
    conn: SqliteConnection,
}

impl ProxyClientsTable {
    fn_table_new!();

    pub fn get_client_by_auth_token(
        &mut self,
        given_auth_token: &str,
    ) -> QueryResult<ProxyClientInfo> {
        use table_schema::proxy_clients::dsl::*;

        // Should use the index on auth token
        proxy_clients
            .filter(auth_token.eq(given_auth_token))
            .first::<ProxyClientInfo>(&mut self.conn)
    }

    // pub fn get_client_auth_token(&mut self, target_client_id: &str) -> Option<String> {
    //     use table_schema::proxy_clients::dsl::*;
    //
    //     proxy_clients
    //         .filter(client_id.eq(target_client_id))
    //         .first::<ProxyClientInfo>(&mut self.conn)
    //         .ok()
    //         .map(|info| info.auth_token)
    // }

    pub fn add_client(&mut self, client_info: &ProxyClientInfo) -> QueryResult<usize> {
        use table_schema::proxy_clients::dsl::*;

        insert_into(proxy_clients)
            .values(client_info)
            .execute(&mut self.conn)
    }

    pub fn get_all_clients(&mut self) -> QueryResult<Vec<ProxyClientInfo>> {
        use table_schema::proxy_clients::dsl::*;

        proxy_clients.load::<ProxyClientInfo>(&mut self.conn)
    }

    pub fn update_data_transfer_info(
        &mut self,
        target_client_id: &str,
        transferred_kb: f64,
    ) -> QueryResult<usize> {
        use table_schema::proxy_clients::dsl::*;

        update(proxy_clients.find(target_client_id))
            .set(data_transferred_kb.eq(data_transferred_kb + transferred_kb))
            .execute(&mut self.conn)
    }

    pub fn update_total_fee_received(
        &mut self,
        target_client_id: &str,
        amount: f64,
    ) -> QueryResult<usize> {
        use table_schema::proxy_clients::dsl::*;

        update(proxy_clients.find(target_client_id))
            .set(total_fee_received.eq(total_fee_received + amount))
            .execute(&mut self.conn)
    }

    pub fn update_total_fee_received_unconfirmed(
        &mut self,
        target_client_id: &str,
        amount: f64,
    ) -> QueryResult<usize> {
        use table_schema::proxy_clients::dsl::*;

        update(proxy_clients.find(target_client_id))
            .set(total_fee_received_unconfirmed.eq(total_fee_received_unconfirmed + amount))
            .execute(&mut self.conn)
    }
}

pub struct ProxySessionsTable {
    conn: SqliteConnection,
}

impl ProxySessionsTable {
    fn_table_new!();

    pub fn get_session_info(&mut self, target_session_id: &str) -> QueryResult<ProxySessionInfo> {
        use table_schema::proxy_sessions::dsl::*;

        proxy_sessions
            .find(target_session_id)
            .first::<ProxySessionInfo>(&mut self.conn)
    }

    pub fn insert_session_info(&mut self, session_info: &ProxySessionInfo) -> QueryResult<usize> {
        use table_schema::proxy_sessions::dsl::*;

        insert_into(proxy_sessions)
            .values(session_info)
            .execute(&mut self.conn)
    }

    pub fn update_data_transfer_info(
        &mut self,
        target_session_id: &str,
        transferred_kb: f64,
    ) -> QueryResult<usize> {
        use table_schema::proxy_sessions::dsl::*;

        update(proxy_sessions.find(target_session_id))
            .set(data_transferred_kb.eq(data_transferred_kb + transferred_kb))
            .execute(&mut self.conn)
    }

    pub fn update_total_fee_sent_unconfirmed(
        &mut self,
        target_session_id: &str,
        amount: f64,
    ) -> QueryResult<usize> {
        use table_schema::proxy_sessions::dsl::*;

        update(proxy_sessions.find(target_session_id))
            .set(total_fee_sent_unconfirmed.eq(total_fee_sent_unconfirmed + amount))
            .execute(&mut self.conn)
    }
}

pub struct PaymentsTable {
    conn: SqliteConnection,
}

impl PaymentsTable {
    fn_table_new!();

    pub fn get_payment_info(&mut self, target_payment_id: &str) -> QueryResult<PaymentInfo> {
        use table_schema::payments::dsl::*;

        payments
            .filter(payment_id.eq(target_payment_id))
            .first::<PaymentInfo>(&mut self.conn)
    }

    pub fn insert_payment_info(&mut self, payment_info: &PaymentInfo) -> QueryResult<usize> {
        use table_schema::payments::dsl::*;

        insert_into(payments)
            .values(payment_info)
            .execute(&mut self.conn)
    }

    pub fn filter_by_status(&mut self, target_status: &str) -> QueryResult<Vec<PaymentInfo>> {
        use table_schema::payments::dsl::*;

        payments
            .filter(status.eq(target_status))
            .load::<PaymentInfo>(&mut self.conn)
    }

    pub fn update_payment_info(&mut self, payment_info: &PaymentInfo) -> QueryResult<usize> {
        use table_schema::payments::dsl::*;

        update(payments.find(payment_info.payment_id.as_str()))
            .set(payment_info)
            .execute(&mut self.conn)
    }
}
