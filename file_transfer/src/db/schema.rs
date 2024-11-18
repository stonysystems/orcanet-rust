use crate::common::HTTPProxyMetadata;
use crate::utils::Utils;
use diesel::{Insertable, Queryable, Selectable};
use serde::Serialize;

pub(super) mod table_schema {
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
            file_size_kb -> Double,
            fee_rate_per_kb -> Nullable<Double>,
            price -> Nullable<Double>,
            payment_tx_id -> Nullable<Text>,
            peer_id -> Text,
            download_timestamp -> BigInt
        }
    }

    diesel::table! {
        proxy_clients (client_id) {
            client_id -> Text,
            auth_token -> Text,
            start_timestamp -> BigInt,
            data_transferred_kb -> Double,
            total_fee_received -> Double,
            total_fee_received_unconfirmed -> Double,
            fee_rate_per_kb -> Double,
            client_peer_id -> Text,
            status -> Integer
        }
    }

    diesel::table! {
        proxy_sessions (session_id) {
            session_id -> Text, // Locally assigned id for internal reference
            client_id -> Text, // Client id assigned by the providing server
            auth_token -> Text,
            proxy_address -> Text,
            start_timestamp -> BigInt,
            end_timestamp -> Nullable<BigInt>,
            data_transferred_kb -> Double,
            total_fee_sent -> Double,
            total_fee_sent_unconfirmed -> Double, // Sent but has not gotten into the blockchain
            fee_rate_per_kb -> Double,
            provider_peer_id -> Text,
            recipient_address -> Text,
            status -> Integer
        }
    }

    diesel::table! {
        kv_store (key) {
            key -> Text,
            value -> Text,
            last_modified -> BigInt,
        }
    }
}

#[derive(Debug, Clone, Serialize, Insertable, Queryable, Selectable)]
#[diesel(table_name = table_schema::provided_files)]
pub struct ProvidedFileInfo {
    pub file_id: String,
    pub file_path: String,
    pub file_name: String,
    pub downloads_count: i32,
    pub status: i32,
    pub provide_start_timestamp: Option<i64>,
}

impl ProvidedFileInfo {
    pub(crate) fn with_defaults(file_id: String, file_path: String, file_name: String) -> Self {
        Self {
            file_id,
            file_path,
            file_name,
            downloads_count: 0,
            status: 1,
            provide_start_timestamp: Some(Utils::get_unix_timestamp()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Insertable, Queryable, Selectable)]
#[diesel(table_name = table_schema::downloaded_files)]
pub struct DownloadedFileInfo {
    pub id: String,
    pub file_id: String,
    pub file_path: String,
    pub file_name: String,
    pub file_size_kb: f64,
    pub fee_rate_per_kb: Option<f64>, // May not be rate but fixed price
    pub price: Option<f64>,           // Size * rate if rate is present
    pub payment_tx_id: Option<String>, // Transaction may not have started, so can be NULL ?
    pub peer_id: String,
    pub download_timestamp: i64,
}

#[derive(PartialEq)]
pub enum ProxySessionStatus {
    Active = 1,
    TerminatedByClient = 0,
    TerminatedByServer = 2,
}

impl TryFrom<i32> for ProxySessionStatus {
    type Error = ();

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::TerminatedByClient),
            1 => Ok(Self::Active),
            2 => Ok(Self::TerminatedByServer),
            _ => Err(()),
        }
    }
}

#[derive(Default, Debug, Clone, Serialize, Insertable, Queryable, Selectable)]
#[diesel(table_name = table_schema::proxy_clients)]
pub struct ProxyClientInfo {
    pub client_id: String,
    pub auth_token: String,
    pub start_timestamp: i64,
    pub data_transferred_kb: f64,
    pub total_fee_received: f64,
    pub total_fee_received_unconfirmed: f64, // Received a transaction, but is not in the blockchain yet
    pub fee_rate_per_kb: f64,
    pub client_peer_id: String,
    pub status: i32, // ProxyStatus
}

impl ProxyClientInfo {
    // TODO: Add client_peer_id
    pub fn with_defaults(client_id: String, auth_token: String, client_peer_id: String) -> Self {
        Self {
            client_id,
            auth_token,
            client_peer_id,
            start_timestamp: Utils::get_unix_timestamp(),
            status: 1,
            ..Self::default()
        }
    }

    pub fn get_fee_owed(&self) -> f64 {
        self.data_transferred_kb * self.fee_rate_per_kb
            - self.total_fee_received
            - self.total_fee_received_unconfirmed
    }
}

#[derive(Default, Debug, Clone, Serialize, Insertable, Queryable, Selectable)]
#[diesel(table_name = table_schema::proxy_sessions)]
pub struct ProxySessionInfo {
    pub session_id: String,
    pub client_id: String,
    pub auth_token: String,
    pub proxy_address: String,
    pub start_timestamp: i64,
    pub end_timestamp: Option<i64>,
    pub data_transferred_kb: f64,
    pub total_fee_sent: f64,
    pub total_fee_sent_unconfirmed: f64,
    pub fee_rate_per_kb: f64,
    pub provider_peer_id: String,
    pub recipient_address: String,
    pub status: i32, // 1 - Active, 0 - terminated by client, -1 - terminated by server
}

impl ProxySessionInfo {
    pub fn from_proxy_connect_response(
        peer_id: String,
        client_id: String,
        auth_token: String,
        metadata: HTTPProxyMetadata,
    ) -> Self {
        Self {
            session_id: Utils::new_uuid(),
            client_id,
            auth_token,
            start_timestamp: Utils::get_unix_timestamp(),
            proxy_address: metadata.proxy_address,
            fee_rate_per_kb: metadata.fee_rate_per_kb,
            provider_peer_id: peer_id,
            recipient_address: metadata.recipient_address,
            status: 1,
            ..Default::default()
        }
    }

    pub fn get_fee_owed(&self) -> f64 {
        self.data_transferred_kb * self.fee_rate_per_kb
            - self.total_fee_sent
            - self.total_fee_sent_unconfirmed
    }
}

#[derive(Debug, Clone, Serialize, Insertable, Queryable, Selectable)]
#[diesel(table_name = table_schema::kv_store)]
pub struct KVStoreRecord {
    pub key: String,
    pub value: String,
    pub last_modified: i64,
}
