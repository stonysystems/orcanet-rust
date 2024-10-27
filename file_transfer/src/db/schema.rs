use diesel::{Insertable, Queryable, Selectable};
use serde::Serialize;

pub mod table_schema {
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

    diesel::table! {
        proxy_clients (client_id) {
            client_id -> Text,
            auth_token -> Text,
            start_timestamp -> BigInt,
            data_transferred_kb -> Float,
            total_fee_received -> Float,
            total_fee_owed -> Float,
            fee_rate_per_kb -> Float,
            last_known_peer_id -> Text,
        }
    }

    diesel::table! {
        kv_store (key) {
            key -> Text,
            value -> Text,
            last_modified -> BigInt,
        }
    }

    // diesel::table! {
    //     proxy_history (orca_token) {
    //         orca_token -> Text,
    //         session_start_time -> BigInt,
    //         peer_id -> Text,
    //         amount_owed -> Integer,
    //         data_transferred_kb -> Float
    //     }
    // }
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

#[derive(Debug, Clone, Serialize, Insertable, Queryable, Selectable)]
#[diesel(table_name = table_schema::downloaded_files)]
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

// #[derive(Debug, Clone, Serialize, Insertable, Queryable, Selectable)]
// #[diesel(table_name = proxy_clients)]
// pub struct ProxyClientInfo {
//     pub file_id: String,
//     pub file_path: String,
//     pub file_name: String,
//     pub downloads_count: i32,
//     pub status: i32,
//     pub provide_start_timestamp: Option<i64>,
// }

#[derive(Debug, Clone, Serialize, Insertable, Queryable, Selectable)]
#[diesel(table_name = table_schema::kv_store)]
pub struct KVStoreRecord {
    pub key: String,
    pub value: String,
    pub last_modified: i64,
}