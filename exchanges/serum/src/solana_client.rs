use crate::market::MarketData;
use anyhow::Result;
use once_cell::sync::Lazy;
use parking_lot::{Mutex, RwLock};

use rand::rngs::OsRng;
use std::collections::HashMap;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;
use serum_dex::matching::Side;

use solana_account_decoder::parse_token::UiTokenAmount;
use solana_account_decoder::{UiAccount, UiAccountEncoding};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
use solana_client::rpc_response::Response;
use solana_program::instruction::Instruction;
use solana_program::pubkey::Pubkey;
use solana_sdk::account::Account;
use solana_sdk::commitment_config::{CommitmentConfig, CommitmentLevel};
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;
use tokio::join;

use mmb_core::connectivity::WebSocketRole;
use mmb_core::exchanges::common::CurrencyPair;
use mmb_core::exchanges::traits::SendWebsocketMessageCb;
use mmb_utils::{impl_u64_id, time::get_atomic_current_secs};

pub const ALLOW_FLAG: bool = false;

pub struct SolanaHosts {
    url: String,
    ws: String,
    market_url: String,
    market_list_json: Option<String>,
}

impl SolanaHosts {
    pub fn new(
        url: String,
        ws: String,
        market_url: String,
        market_list_json: Option<String>,
    ) -> Self {
        SolanaHosts {
            url,
            ws,
            market_url,
            market_list_json,
        }
    }
}

pub enum NetworkType {
    Mainnet,
    Custom(SolanaHosts),
}

impl NetworkType {
    pub fn url(&self) -> &str {
        match self {
            NetworkType::Mainnet => "https://api.mainnet-beta.solana.com",
            NetworkType::Custom(network_opts) => &network_opts.url,
        }
    }

    pub fn ws(&self) -> &str {
        match self {
            NetworkType::Mainnet => "ws://api.mainnet-beta.solana.com/",
            NetworkType::Custom(network_opts) => &network_opts.ws,
        }
    }

    pub fn market_list_url(&self) -> &str {
        match self {
            NetworkType::Custom(network_opts) => &network_opts.market_url,
            _ => "https://raw.githubusercontent.com/project-serum/serum-ts/master/packages/serum/src/markets.json",
        }
    }

    pub fn market_list_json(&self) -> Option<&String> {
        match self {
            NetworkType::Custom(network_opts) => network_opts.market_list_json.as_ref(),
            _ => None,
        }
    }
}

#[derive(Deserialize, Debug)]
struct SubscribeResult {
    id: RequestId,
    result: RequestId,
}

#[derive(Deserialize, Debug)]
struct AccountNotification {
    params: NotificationParams,
}

#[derive(Deserialize, Debug)]
struct NotificationParams {
    result: Response<UiAccount>,
    subscription: RequestId,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum WebsocketMessage {
    SubscribeResult(SubscribeResult),
    AccountNotification(AccountNotification),
}

#[derive(Debug, Clone, Copy)]
pub enum SubscriptionAccountType {
    OrderBook,
    EventQueue,
    OpenOrders,
}

#[derive(Debug, Clone)]
struct SubscriptionMarketData {
    currency_pair: CurrencyPair,
    side: Side,
    account_type: SubscriptionAccountType,
}

impl_u64_id!(RequestId);

pub enum SolanaMessage {
    Unknown,
    Service,
    AccountUpdated(CurrencyPair, Side, UiAccount, SubscriptionAccountType),
}

/// Wrapper for the solana rpc client with support for asynchronous methods
/// and subscription to order change events
pub struct SolanaClient {
    rpc_client: Arc<RpcClient>,
    send_websocket_message_callback: Mutex<SendWebsocketMessageCb>,
    subscription_requests: RwLock<HashMap<RequestId, SubscriptionMarketData>>,
    subscriptions: RwLock<HashMap<RequestId, SubscriptionMarketData>>,
}

impl SolanaClient {
    pub fn new(network_type: &NetworkType) -> Self {
        let async_rpc_client = RpcClient::new(network_type.url().to_string());

        Self {
            rpc_client: Arc::new(async_rpc_client),
            send_websocket_message_callback: Mutex::new(Box::new(|_, _| {
                Err(anyhow::anyhow!("not connected!"))
            })),
            subscription_requests: Default::default(),
            subscriptions: Default::default(),
        }
    }

    pub fn set_send_websocket_message_callback(&self, callback: SendWebsocketMessageCb) {
        *self.send_websocket_message_callback.lock() = callback;
    }

    pub async fn get_account(&self, pubkey: &Pubkey) -> Result<Account> {
        self.rpc_client
            .get_account(pubkey)
            .await
            .map_err(|err| err.into())
    }

    pub async fn get_account_data(&self, pubkey: &Pubkey) -> Result<Vec<u8>> {
        self.rpc_client
            .get_account_data(pubkey)
            .await
            .map_err(|err| err.into())
    }

    pub async fn get_program_accounts_with_config(
        &self,
        pubkey: &Pubkey,
        config: RpcProgramAccountsConfig,
    ) -> Result<Vec<(Pubkey, Account)>> {
        self.rpc_client
            .get_program_accounts_with_config(pubkey, config)
            .await
            .map_err(|err| err.into())
    }

    pub async fn get_token_account_balance(&self, pubkey: &Pubkey) -> Result<UiTokenAmount> {
        self.rpc_client
            .get_token_account_balance(pubkey)
            .await
            .map_err(|err| err.into())
    }

    pub async fn send_instructions(
        &self,
        payer: &Keypair,
        instructions: &[Instruction],
    ) -> Result<()> {
        let recent_hash = self.rpc_client.get_latest_blockhash().await?;
        let transaction = Transaction::new_signed_with_payer(
            instructions,
            Some(&payer.pubkey()),
            &[payer],
            recent_hash,
        );

        self.rpc_client.send_transaction(&transaction).await?;
        Ok(())
    }

    pub async fn create_dex_account(
        &self,
        program_id: &Pubkey,
        payer: &Pubkey,
        length: usize,
    ) -> Result<(Keypair, Instruction)> {
        let key = Keypair::generate(&mut OsRng);
        let lamports = self
            .rpc_client
            .get_minimum_balance_for_rent_exemption(length)
            .await?;

        let create_account_instr = solana_sdk::system_instruction::create_account(
            payer,
            &key.pubkey(),
            lamports,
            length as u64,
            program_id,
        );
        Ok((key, create_account_instr))
    }

    pub async fn subscribe_to_market(&self, currency_pair: &CurrencyPair, market: &MarketData) {
        let market_info = market.metadata;

        let ask_request_id = RequestId::generate();
        self.subscription_requests.write().insert(
            ask_request_id,
            SubscriptionMarketData {
                currency_pair: *currency_pair,
                side: Side::Ask,
                account_type: SubscriptionAccountType::OrderBook,
            },
        );

        let bid_request_id = RequestId::generate();
        self.subscription_requests.write().insert(
            bid_request_id,
            SubscriptionMarketData {
                currency_pair: *currency_pair,
                side: Side::Bid,
                account_type: SubscriptionAccountType::OrderBook,
            },
        );

        let event_queue_request_id = RequestId::generate();
        self.subscription_requests.write().insert(
            event_queue_request_id,
            SubscriptionMarketData {
                currency_pair: *currency_pair,
                side: Side::Bid,
                account_type: SubscriptionAccountType::EventQueue,
            },
        );

        join!(
            self.subscribe_to_address_changed(ask_request_id, &market_info.asks_address),
            self.subscribe_to_address_changed(bid_request_id, &market_info.bids_address),
            self.subscribe_to_address_changed(
                event_queue_request_id,
                &market_info.event_queue_address
            )
        );
    }

    pub async fn subscribe_to_open_order_account(
        &self,
        currency_pair: &CurrencyPair,
        pubkey: Pubkey,
    ) {
        let wallet_request_id = RequestId::generate();
        self.subscription_requests.write().insert(
            wallet_request_id,
            SubscriptionMarketData {
                currency_pair: *currency_pair,
                side: Side::Bid,
                account_type: SubscriptionAccountType::OpenOrders,
            },
        );

        self.subscribe_to_address_changed(wallet_request_id, &pubkey)
            .await;
    }

    pub fn handle_on_message(&self, message: &str) -> SolanaMessage {
        let message: WebsocketMessage = match serde_json::from_str(message) {
            Ok(message) => message,
            Err(err) => {
                log::warn!("Unknown message type. {}. Message: {}", err, message);
                return SolanaMessage::Unknown;
            }
        };

        match message {
            WebsocketMessage::SubscribeResult(subscribe_result) => {
                if let Some(subscription_market_data) = self
                    .subscription_requests
                    .write()
                    .remove(&subscribe_result.id)
                {
                    self.subscriptions
                        .write()
                        .insert(subscribe_result.result, subscription_market_data);
                } else {
                    // It is possible when we receive a message before subscribe was completed on Solana side
                    // Non-critical so we just logging it
                    // If we have not been subscribed to account yet we should think that all its messages are not for us
                    log::trace!("Subscription was not found for id {}", subscribe_result.id);
                }
                SolanaMessage::Service
            }
            WebsocketMessage::AccountNotification(account_notification) => {
                let subscription_id = account_notification.params.subscription;
                let read_guard = self.subscriptions.read();
                if let Some(subscription_market_data) = read_guard.get(&subscription_id) {
                    SolanaMessage::AccountUpdated(
                        subscription_market_data.currency_pair,
                        subscription_market_data.side,
                        account_notification.params.result.value,
                        subscription_market_data.account_type,
                    )
                } else {
                    // It is possible when we receive a message before subscribe was completed on Solana side
                    // Non-critical so we just logging it
                    // If we have not been subscribed to account yet we should think that all its messages are not for us
                    log::trace!("Subscription was not found for id {}", subscription_id);
                    SolanaMessage::Unknown
                }
            }
        }
    }

    async fn subscribe_to_address_changed(&self, request_id: RequestId, pubkey: &Pubkey) {
        let config = Some(RpcAccountInfoConfig {
            encoding: Some(UiAccountEncoding::JsonParsed),
            commitment: Some(CommitmentConfig {
                commitment: CommitmentLevel::Confirmed,
            }),
            data_slice: None,
        });

        let message = json!({
            "jsonrpc":"2.0",
            "id":request_id,
            "method":"accountSubscribe",
            "params":[
                pubkey.to_string(),
                config
            ]
        })
        .to_string();

        self.send_websocket_message_callback.lock()(WebSocketRole::Main, message)
            .expect("failed to send websocket message")
    }
}
