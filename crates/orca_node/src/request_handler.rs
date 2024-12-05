use std::collections::HashMap;
use std::path::Path;

use futures::channel::mpsc;
use futures::{SinkExt, StreamExt};
use libp2p::PeerId;
use serde_json::json;
use tokio::select;
use tracing_subscriber::filter::FilterExt;

use crate::common::config::{ConfigKey, OrcaNetConfig};
use crate::common::types::{
    FileMetadata, HTTPProxyMetadata, OrcaNetError, OrcaNetEvent, OrcaNetRequest, OrcaNetResponse,
    PaymentRequest, PrePaymentResponse, ProxyMode,
};
use crate::common::Utils;
use crate::db::{
    PaymentCategory, PaymentInfo, PaymentStatus, PaymentsTable, ProvidedFileInfo,
    ProvidedFilesTable, ProxyClientInfo, ProxyClientsTable,
};
use crate::network::NetworkClient;
use crate::proxy::start_http_proxy;

pub struct RequestHandlerLoop {
    network_client: NetworkClient,
    event_receiver: mpsc::Receiver<OrcaNetEvent>,
    proxy_event_sender: Option<mpsc::Sender<OrcaNetEvent>>,
}

impl RequestHandlerLoop {
    pub fn new(
        network_client: NetworkClient,
        event_receiver: mpsc::Receiver<OrcaNetEvent>,
    ) -> Self {
        RequestHandlerLoop {
            network_client,
            event_receiver,
            proxy_event_sender: None,
        }
    }

    pub async fn run(mut self) {
        // Start providing all configured files
        // TODO

        loop {
            select! {
                event = self.event_receiver.next() => match event {
                    Some(ev) => self.handle_event(ev).await,
                    _ => {todo!("Not implemented")}
                }
            }
        }
    }

    async fn handle_event(&mut self, event: OrcaNetEvent) {
        match event {
            OrcaNetEvent::Request {
                request,
                from_peer,
                channel,
            } => {
                let response = self.handle_request(request, from_peer).unwrap_or_else(|e| {
                    tracing::error!("Error while handling orcanet request: {:?}", e);
                    OrcaNetResponse::Error(OrcaNetError::InternalServerError(e.to_string()))
                });
                self.network_client.respond(response, channel).await;
            }
            OrcaNetEvent::StreamRequest {
                request,
                from_peer,
                sender,
            } => {
                let response = self.handle_request(request, from_peer).unwrap_or_else(|e| {
                    tracing::error!("Error while handling request: {:?}", e);
                    OrcaNetResponse::Error(OrcaNetError::InternalServerError(e.to_string()))
                });
                let _ = sender.send(response);
            }
            OrcaNetEvent::ProvideFile { file_id, file_path } => {
                let path = Path::new(&file_path);
                let file_name: String = Utils::get_file_name_from_path(&path);

                if path.exists() {
                    let mut provided_files_table = ProvidedFilesTable::new(None);
                    let file_info =
                        ProvidedFileInfo::with_defaults(file_id.clone(), file_path, file_name);
                    let resp = provided_files_table.insert_provided_file(&file_info);

                    match resp {
                        Ok(_) => self.network_client.start_providing(file_id).await,
                        Err(e) => {
                            tracing::error!("Failed to insert into DB: {:?}", e);
                        }
                    }
                }
            }
            OrcaNetEvent::StopProvidingFile { file_id, permanent } => {
                if permanent {
                    // Permanently stop providing - Remove from DB
                    let mut provided_files_table = ProvidedFilesTable::new(None);
                    let remove_resp = provided_files_table.remove_provided_file(file_id.as_str());

                    if let Err(e) = remove_resp {
                        tracing::error!("Deletion failed for {}. Error: {:?}", file_id.as_str(), e)
                    }
                } else {
                    let mut provided_files_table = ProvidedFilesTable::new(None);
                    let status_change_resp = provided_files_table.set_provided_file_status(
                        file_id.as_str(),
                        false,
                        None,
                    );

                    if let Err(e) = status_change_resp {
                        tracing::error!(
                            "Error changing status for {}. Error: {:?}",
                            file_id.as_str(),
                            e
                        )
                    }
                }

                self.network_client.stop_providing(file_id).await;
            }
            // OrcaNetEvent::ChangeProxyClient(client_config) => {
            //     // We restart to change proxy configuration
            //     // Technically, we only need to change the address and port in the running client
            //     // But that requires mutating it, and we need to start using Mutex locks for handler
            //     // Since changing is expected to be rare, I don't think it's worth introducing locks
            //     // TODO: Change later if required
            //
            //     // About why Box::pin is needed: https://rust-lang.github.io/async-book/07_workarounds/04_recursion.html
            //     Box::pin(self.handle_event(OrcaNetEvent::StopProxy)).await;
            //     Box::pin(
            //         self.handle_event(OrcaNetEvent::StartProxy(ProxyMode::ProxyClient(
            //             client_config,
            //         ))),
            //     )
            //     .await;
            // }
            OrcaNetEvent::StartProxy(proxy_mode) => {
                if self.proxy_event_sender.is_some() {
                    println!("Proxy server already running");
                    return;
                }

                let (proxy_event_sender, proxy_event_receiver) = mpsc::channel::<OrcaNetEvent>(0);
                tokio::task::spawn(start_http_proxy(
                    proxy_mode.clone(),
                    self.network_client.clone(),
                    proxy_event_receiver,
                ));
                self.proxy_event_sender = Some(proxy_event_sender);

                match &proxy_mode {
                    ProxyMode::ProxyProvider => {
                        tracing::info!("Started proxy provider. Putting provider record");
                        self.network_client
                            .start_providing(OrcaNetConfig::PROXY_PROVIDER_KEY_DHT.to_string())
                            .await;
                    }
                    ProxyMode::ProxyClient { session_id } => {
                        tracing::info!("Started proxy client with session_id: {:?}", session_id);
                    }
                }

                OrcaNetConfig::modify_config(
                    ConfigKey::ProxyConfig.to_string().as_str(),
                    json!(Some(proxy_mode)),
                )
                .expect("Proxy configuration to be updated");
            }
            OrcaNetEvent::StopProxy => {
                let proxy_mode =
                    OrcaNetConfig::get_proxy_config().expect("Proxy mode to be present in config");

                Self::send_event_to_proxy_server(
                    OrcaNetEvent::StopProxy,
                    self.proxy_event_sender.take().as_mut(),
                )
                .await;

                match proxy_mode {
                    ProxyMode::ProxyProvider => {
                        self.network_client
                            .stop_providing(OrcaNetConfig::PROXY_PROVIDER_KEY_DHT.to_string())
                            .await;
                    }
                    ProxyMode::ProxyClient { session_id } => {
                        // TODO: Pay remaining amount owed
                    }
                }

                OrcaNetConfig::modify_config(
                    ConfigKey::ProxyConfig.to_string().as_str(),
                    json!(None::<ProxyMode>),
                )
                .expect("Proxy configuration to be updated");
            }
        }
    }

    async fn send_event_to_proxy_server(
        event: OrcaNetEvent,
        sender: Option<&mut mpsc::Sender<OrcaNetEvent>>,
    ) {
        // TODO: Return responses instead of just logging
        match sender {
            Some(proxy_event_sender) => {
                if let Err(e) = proxy_event_sender.send(event).await {
                    tracing::error!("Error sending command to proxy: {:?}", e);
                }
            }
            None => {
                tracing::error!("Send event to proxy: Proxy server not running");
            }
        }
    }

    fn handle_request(
        &mut self,
        request: OrcaNetRequest,
        from_peer: PeerId,
    ) -> Result<OrcaNetResponse, Box<dyn std::error::Error>> {
        match request {
            OrcaNetRequest::FileMetadataRequest { .. }
            | OrcaNetRequest::FileContentRequest { .. } => {
                Self::handle_file_request(request, from_peer)
            }
            OrcaNetRequest::HTTPProxyMetadataRequest | OrcaNetRequest::HTTPProxyProvideRequest => {
                Self::handle_http_proxy_request(request, from_peer)
            }
            OrcaNetRequest::HTTPProxyPrePaymentRequest { .. }
            | OrcaNetRequest::HTTPProxyPostPaymentNotification { .. } => {
                Self::handle_payment_request(request, from_peer)
            }
        }
    }

    fn handle_file_request(
        request: OrcaNetRequest,
        from_peer: PeerId,
    ) -> Result<OrcaNetResponse, Box<dyn std::error::Error>> {
        let file_id = match &request {
            OrcaNetRequest::FileMetadataRequest { file_id } => file_id,
            OrcaNetRequest::FileContentRequest { file_id } => file_id,
            _ => panic!("Expected file request"),
        };

        let mut provided_files_table = ProvidedFilesTable::new(None);
        let file_info_resp = provided_files_table.get_provided_file_info(file_id.as_str());

        if file_info_resp.is_err() {
            // Most likely not present in DB
            tracing::error!("Requested file not found in DB");
            return Ok(OrcaNetResponse::Error(OrcaNetError::NotAProvider(
                "Not a provider of requested file".to_string(),
            )));
        }

        let file_info = file_info_resp.unwrap();
        tracing::info!("File info for request {} {:?}", file_id, file_info);

        let file_metadata = FileMetadata {
            file_id: file_id.clone(),
            file_name: file_info.file_name,
            fee_rate_per_kb: OrcaNetConfig::get_file_fee_rate(),
            recipient_address: OrcaNetConfig::get_btc_address(),
        };

        match request {
            OrcaNetRequest::FileMetadataRequest { .. } => {
                tracing::info!("Received metadata request for file_id: {}", file_id);

                Ok(OrcaNetResponse::FileMetadataResponse(file_metadata))
            }
            OrcaNetRequest::FileContentRequest { .. } => {
                tracing::info!("Received content request for file_id: {}", file_id);
                let content = std::fs::read(&file_info.file_path)?;
                let _ = provided_files_table.increment_download_count(file_id.as_str());

                Ok(OrcaNetResponse::FileContentResponse {
                    metadata: file_metadata,
                    content,
                })
            }
            _ => panic!("Expected file request"),
        }
    }

    fn handle_http_proxy_request(
        request: OrcaNetRequest,
        from_peer: PeerId,
    ) -> Result<OrcaNetResponse, Box<dyn std::error::Error>> {
        match OrcaNetConfig::get_proxy_config() {
            Some(ProxyMode::ProxyProvider) => {
                let metadata = HTTPProxyMetadata {
                    proxy_address: OrcaNetConfig::get_str_from_config(
                        ConfigKey::ProxyProviderServerAddress,
                    ),
                    fee_rate_per_kb: OrcaNetConfig::get_proxy_fee_rate(),
                    recipient_address: OrcaNetConfig::get_btc_address(),
                };

                match request {
                    OrcaNetRequest::HTTPProxyMetadataRequest => {
                        Ok(OrcaNetResponse::HTTPProxyMetadataResponse(metadata))
                    }
                    OrcaNetRequest::HTTPProxyProvideRequest => {
                        // Create new client
                        let client_id = Utils::new_uuid();
                        let auth_token = Utils::new_uuid();

                        let mut proxy_clients_table = ProxyClientsTable::new(None);
                        let proxy_client_info = ProxyClientInfo::with_defaults(
                            client_id.clone(),
                            auth_token.clone(),
                            from_peer.to_string(),
                        );
                        proxy_clients_table.add_client(&proxy_client_info)?;

                        Ok(OrcaNetResponse::HTTPProxyProvideResponse {
                            metadata,
                            client_id,
                            auth_token,
                        })
                    }
                    _ => panic!("Expected only proxy requests in handle_proxy_requests"),
                }
            }
            _ => {
                // Not providing
                Ok(OrcaNetResponse::Error(OrcaNetError::NotAProvider(
                    "Not a proxy provider".to_string(),
                )))
            }
        }
    }

    fn handle_payment_request(
        request: OrcaNetRequest,
        from_peer: PeerId,
    ) -> Result<OrcaNetResponse, Box<dyn std::error::Error>> {
        match request {
            OrcaNetRequest::HTTPProxyPrePaymentRequest {
                client_id,
                auth_token,
                fee_owed: fee_owed_client,
                data_transferred_kb: data_transferred_kb_client,
            } => {
                let mut proxy_clients_table = ProxyClientsTable::new(None);
                let client_info =
                    proxy_clients_table.get_client_by_auth_token(auth_token.as_str())?;
                let fee_owed = client_info.get_fee_owed();
                let fee_owed_percent_diff = Utils::get_percent_diff(fee_owed_client, fee_owed);
                let data_trans_percent_diff = Utils::get_percent_diff(
                    data_transferred_kb_client,
                    client_info.data_transferred_kb,
                );

                // Verify the client's claim about data transfer and fee owed
                // Accept if they are within acceptable range, reject otherwise
                let pre_payment_response = {
                    if fee_owed_percent_diff > OrcaNetConfig::PROXY_TERMINATION_PD_THRESHOLD
                        || data_trans_percent_diff > OrcaNetConfig::PROXY_TERMINATION_PD_THRESHOLD
                    {
                        // If either differ too much, terminate the connection
                        PrePaymentResponse::Accepted(PaymentRequest {
                            payment_reference: Utils::new_uuid(),
                            amount_to_send: fee_owed,
                            recipient_address: OrcaNetConfig::get_btc_address(),
                        })
                    } else if fee_owed_percent_diff > OrcaNetConfig::FEE_OWED_PD_ALLOWED {
                        PrePaymentResponse::RejectedDataTransferDiffers
                    } else if data_trans_percent_diff > OrcaNetConfig::DATA_TRANSFER_PD_ALLOWED {
                        PrePaymentResponse::RejectedFeeOwedDiffers
                    } else {
                        let payment_reference = Utils::new_uuid();
                        let btc_address = OrcaNetConfig::get_btc_address();
                        // TODO: May be create a random float between fee_owed and say 90% of fee_owed ?
                        // This would reduce the chances of cheating, i.e double spend where a client
                        // tries to report a previously sent transaction for a new payment cuz we
                        // request a specific amount that a previous transaction may not have
                        let amount_to_send = fee_owed;

                        // TODO: Persist in DB
                        let mut payments_table = PaymentsTable::new(None);
                        let payment_info = PaymentInfo {
                            payment_id: payment_reference.clone(),
                            to_address: btc_address.clone(),
                            expected_amount_btc: Some(amount_to_send),
                            category: PaymentCategory::Receive.to_string(),
                            status: PaymentStatus::AwaitingClientConfirmation.to_string(),
                            from_peer: Some(from_peer.to_string()),
                            ..Default::default()
                        };

                        payments_table.insert_payment_info(&payment_info)?;

                        PrePaymentResponse::Accepted(PaymentRequest {
                            payment_reference,
                            amount_to_send,
                            recipient_address: OrcaNetConfig::get_btc_address(),
                        })
                    }
                };

                Ok(OrcaNetResponse::HTTPProxyPrePaymentResponse {
                    data_transferred_kb: client_info.data_transferred_kb,
                    fee_owed,
                    pre_payment_response,
                })
            }
            OrcaNetRequest::HTTPProxyPostPaymentNotification {
                client_id,
                auth_token,
                payment_notification,
            } => {
                let mut proxy_clients_table = ProxyClientsTable::new(None);
                let client_info =
                    proxy_clients_table.get_client_by_auth_token(auth_token.as_str())?;

                let mut payments_table = PaymentsTable::new(None);
                // TODO: Verify tx_id doesnt already exist in the database
                // This means the client is trying to double spend a transaction with us

                let mut payment_info = match payments_table
                    .get_payment_info(payment_notification.payment_reference.as_str())
                {
                    Ok(payment_info) => payment_info,
                    Err(_) => {
                        // Client should send us a payment reference we gave it
                        return Ok(OrcaNetResponse::Error(
                            OrcaNetError::PaymentReferenceMismatch,
                        ));
                    }
                };

                // Update payment info
                payment_info.tx_id = Some(payment_notification.tx_id);
                payment_info.from_address = Some(payment_notification.sender_address);
                payment_info.status = PaymentStatus::TransactionPending.to_string();
                payments_table.update_payment_info(&payment_info)?;

                // Update client info
                // This is temporary - Must be changed if we wait and find the transaction is fake
                // We do this so that if it takes some time for the transaction to reach us and our verification to complete,
                // we don't terminate the connection unnecessarily for a honest client
                proxy_clients_table.update_total_fee_received_unconfirmed(
                    client_info.client_id.as_str(),
                    payment_notification.amount_transferred,
                )?;

                // Assume the client is honest and sent you a valid transaction
                // It may take a while for the transaction to reach this node

                // Handle the dishonesty case in which the client gave you a random tx that doesn't exist
                // May be handle that in a separate thread that reads the DB, checks unconfirmed transactions,
                // marks confirmed ones, blacklists cheating clients etc
                // todo!()
                Ok(OrcaNetResponse::NullResponse)
            }
            _ => panic!("Expected payment request"),
        }
    }
}
