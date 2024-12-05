use crate::impl_str_serde;
use futures::channel::oneshot;
use libp2p::request_response::ResponseChannel;
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::error::Error;
use std::io::Write;

#[derive(Debug)]
pub enum NetworkCommand {
    StartListening {
        addr: Multiaddr,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>,
    },
    Dial {
        peer_id: PeerId,
        peer_addr: Multiaddr,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>,
    },
    StartProviding {
        key: String,
        sender: oneshot::Sender<()>,
    },
    StopProviding {
        key: String,
    },
    GetProviders {
        key: String,
        sender: oneshot::Sender<HashSet<PeerId>>,
    },
    PutKV {
        key: String,
        value: Vec<u8>,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>,
    },
    GetValue {
        key: String,
        sender: oneshot::Sender<Result<Vec<u8>, Box<dyn Error + Send>>>,
    },
    Request {
        request: OrcaNetRequest,
        peer: PeerId,
        sender: oneshot::Sender<Result<OrcaNetResponse, Box<dyn Error + Send>>>,
    },
    Respond {
        response: OrcaNetResponse,
        channel: ResponseChannel<OrcaNetResponse>,
    },
    SendInStream {
        peer_id: PeerId,
        stream_req: StreamReq,
        sender: Option<oneshot::Sender<Result<OrcaNetResponse, Box<dyn Error + Send>>>>,
    },
}

#[derive(Debug)]
pub enum OrcaNetEvent {
    Request {
        request: OrcaNetRequest,
        from_peer: PeerId,
        channel: ResponseChannel<OrcaNetResponse>,
    },
    StreamRequest {
        request: OrcaNetRequest,
        from_peer: PeerId,
        sender: oneshot::Sender<OrcaNetResponse>,
    },
    ProvideFile {
        file_id: String,
        file_path: String,
    },
    StopProvidingFile {
        file_id: String,
        permanent: bool, // Permanently stop providing
    },
    StartProxy(ProxyMode), // TODO: Add response sender
    StopProxy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamReq {
    pub request_id: String,
    pub stream_data: StreamData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamData {
    Request(OrcaNetRequest),
    Response(OrcaNetResponse),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentNotification {
    /// Bitcoin address of the sender
    pub sender_address: String,
    /// Should be bitcoin address of the server
    pub receiver_address: String,
    pub amount_transferred: f64,
    /// Transaction ID in the blockchain for the transaction created by the sender
    pub tx_id: String,
    /// Server generated payment reference if provided
    pub payment_reference: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrcaNetRequest {
    FileMetadataRequest {
        file_id: String,
    },
    FileContentRequest {
        file_id: String,
    },
    HTTPProxyMetadataRequest,
    HTTPProxyProvideRequest,
    /// To see if server and client agree on the data transferred and amount owed
    HTTPProxyPrePaymentRequest {
        client_id: String,
        auth_token: String,
        data_transferred_kb: f64,
        fee_owed: f64,
    },
    HTTPProxyPostPaymentNotification {
        client_id: String,
        auth_token: String,
        payment_notification: PaymentNotification,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HTTPProxyMetadata {
    pub proxy_address: String, // IP address with port
    pub fee_rate_per_kb: f64,
    pub recipient_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub file_id: String,
    pub file_name: String,
    pub fee_rate_per_kb: f64,
    pub recipient_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentRequest {
    pub amount_to_send: f64, // Server requests a specific amount <= amount_owed
    pub payment_reference: String,
    pub recipient_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PrePaymentResponse {
    Accepted(PaymentRequest),
    RejectedDataTransferDiffers,
    RejectedFeeOwedDiffers,
    /// Server wants to terminate the connection and requests a final payment
    ServerTerminatingConnection(PaymentRequest),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrcaNetResponse {
    FileMetadataResponse(FileMetadata),
    FileContentResponse {
        metadata: FileMetadata,
        content: Vec<u8>,
    },
    HTTPProxyMetadataResponse(HTTPProxyMetadata),
    HTTPProxyProvideResponse {
        metadata: HTTPProxyMetadata,
        client_id: String,
        auth_token: String,
    },
    HTTPProxyPrePaymentResponse {
        /// Data transferred according to server
        data_transferred_kb: f64,
        /// Amount owed according to server
        fee_owed: f64,
        pre_payment_response: PrePaymentResponse, // TODO: Think of a better name
    },
    Error(OrcaNetError),
    NullResponse, // For testing
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "message")]
pub enum OrcaNetError {
    AuthorizationFailed(String),
    NotAProvider(String),
    InternalServerError(String),
    PaymentReferenceMismatch,
    SessionTerminatedByProvider,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyClientConfig {
    pub session_id: String,
    pub proxy_address: String,
    pub client_id: String,
    pub auth_token: String,
    pub fee_rate_per_kb: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "message")]
pub enum ProxyMode {
    ProxyProvider,
    ProxyClient { session_id: String },
}
