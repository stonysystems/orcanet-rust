use crate::btc_rpc::RPCWrapper;
use crate::common::{
    OrcaNetConfig, OrcaNetRequest, OrcaNetResponse, PaymentNotification, PaymentRequest,
    PrePaymentResponse,
};
use crate::db::{
    PaymentCategory, PaymentInfo, PaymentStatus, PaymentsTable, ProxySessionInfo,
    ProxySessionsTable,
};
use crate::network_client::NetworkClient;
use crate::utils::Utils;
use libp2p::PeerId;
use std::time::Duration;
use tokio::time::interval;
use tokio_util::sync::CancellationToken;

pub struct ProxyPaymentLoop {
    pub session_id: String,
    pub network_client: NetworkClient,
    pub proxy_sessions_table: ProxySessionsTable,
    pub cancellation_token: CancellationToken,
}

impl ProxyPaymentLoop {
    pub async fn start_loop(mut self) {
        let mut interval = interval(Duration::from_secs(
            OrcaNetConfig::PROXY_PAYMENT_INTERVAL_SECS,
        ));

        loop {
            tokio::select! {
                _ = self.cancellation_token.cancelled() => {
                    tracing::info!("Received loop cancellation");
                    // Settle up
                    // Cancel payment loop
                    return;
                }

                interval_event = interval.tick() => {
                    tracing::info!("Attempting payment for HTTP proxy");

                    match self.process_payment().await {
                        Ok(payment_reference) => {
                            if payment_reference.is_some() {
                                 tracing::info!(
                                "Payment attempt succeeded. Reference: {:?}",
                                payment_reference
                                );
                            }
                        }
                        Err(e) => {
                            tracing::error!("Payment attempt failed: {:?}", e);
                        }
                    }
                }
            }
        }
    }

    async fn process_payment(&mut self) -> Result<Option<String>, Box<dyn std::error::Error>> {
        // Get session info
        let mut proxy_sessions_table = ProxySessionsTable::new(None);
        let session_info = proxy_sessions_table.get_session_info(self.session_id.as_str())?;

        if session_info.get_fee_owed() == 0f64 {
            tracing::info!("No pending amount. Payment not required.");
            return Ok(None);
        }

        tracing::info!(
            "Processing payment for {} Fee owed {}",
            session_info.session_id,
            session_info.get_fee_owed()
        );

        let provider_peer = session_info
            .provider_peer_id
            .parse()
            .expect("Provider peer id to be valid");

        // Send pre-payment request to make sure the server agrees and to get amount to be paid
        let payment_request = self.pre_payment_step(provider_peer, &session_info).await?;

        // Create a transaction for the requested amount
        let rpc_wrapper = RPCWrapper::new(OrcaNetConfig::get_network_type());
        let comment = format!(
            "Payment for proxy. Reference: {:?}",
            payment_request.payment_reference
        );
        let tx_id = rpc_wrapper.send_to_address(
            payment_request.recipient_address.as_str(),
            payment_request.amount_to_send,
            None,
        )?;

        // Persist in database
        let mut payments_table = PaymentsTable::new(None);
        let payment_info = PaymentInfo {
            payment_id: Utils::new_uuid(), // To make sure server doesn't mess up our payment by giving old/existing reference
            tx_id: Some(tx_id.to_string()),
            to_address: payment_request.recipient_address.clone(),
            amount_btc: Some(payment_request.amount_to_send),
            category: PaymentCategory::Send.to_string(),
            status: PaymentStatus::TransactionPending.to_string(),
            payment_reference: Some(payment_request.payment_reference.clone()),
            from_peer: None,
            to_peer: Some(session_info.provider_peer_id),
            ..Default::default()
        };
        payments_table.insert_payment_info(&payment_info)?;

        tracing::info!("Inserted payment record");

        let mut sessions_table = ProxySessionsTable::new(None);
        sessions_table.update_total_fee_sent_unconfirmed(
            self.session_id.as_str(),
            payment_request.amount_to_send,
        )?;

        tracing::info!("Updated session record");

        // Send post payment notification to the server
        let res = self
            .network_client
            .send_request(
                provider_peer,
                OrcaNetRequest::HTTPProxyPostPaymentNotification {
                    client_id: session_info.client_id.clone(),
                    auth_token: session_info.auth_token.clone(),
                    payment_notification: PaymentNotification {
                        sender_address: OrcaNetConfig::get_btc_address(),
                        receiver_address: payment_request.recipient_address,
                        amount_transferred: payment_request.amount_to_send,
                        tx_id: tx_id.to_string(),
                        payment_reference: payment_request.payment_reference.clone(),
                    },
                },
            )
            .await;

        tracing::info!("Sent post payment notification");

        Ok(Some(payment_request.payment_reference))
    }

    async fn pre_payment_step(
        &mut self,
        peer_id: PeerId,
        session_info: &ProxySessionInfo,
    ) -> Result<PaymentRequest, Box<dyn std::error::Error>> {
        let resp = self
            .network_client
            .send_request(
                peer_id,
                OrcaNetRequest::HTTPProxyPrePaymentRequest {
                    client_id: session_info.client_id.clone(),
                    auth_token: session_info.auth_token.clone(),
                    fee_owed: session_info.get_fee_owed(),
                    data_transferred_kb: session_info.data_transferred_kb,
                },
            )
            .await
            .map_err(|e| e as Box<dyn std::error::Error>)?;

        match resp {
            OrcaNetResponse::HTTPProxyPrePaymentResponse {
                fee_owed,
                data_transferred_kb,
                pre_payment_response,
            } => {
                tracing::info!(
                    "Received pre payment response {:?}. Data transferred: {}, fee_owed: {}",
                    pre_payment_response,
                    data_transferred_kb,
                    fee_owed
                );

                // Check if server's data differs too much
                // if data_transferred_kb != session_info.data_transferred_kb {
                //
                // }

                // If it does, send all remaining amount and terminate the connection
                // TODO: Note down the incident so we can factor this into reputation calculation

                // If not, proceed to handle the server's response
                match pre_payment_response {
                    PrePaymentResponse::Accepted(payment_req) => Ok(payment_req),
                    PrePaymentResponse::RejectedDataTransferDiffers => {
                        // Adjust to what the server says
                        // At this point, we've decided that the server's values are not too different, so it's fine
                        todo!()
                    }
                    PrePaymentResponse::RejectedFeeOwedDiffers => {
                        // Adjust to what the server says
                        todo!()
                    }
                    PrePaymentResponse::ServerTerminatingConnection(payment_req) => {
                        todo!()
                    }
                }
            }
            OrcaNetResponse::Error(e) => {
                Err(format!("Got error for pre payment request {:?}", e).into())
            }
            // Err(e) => Err(format!("Got error for pre payment request {:?}", e).into()),
            _ => Err("Got invalid response for pre payment request".into()),
        }
    }
}
