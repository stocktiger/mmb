use super::binance::Binance;
use crate::support::{BinanceOrderInfo, BinancePosition};
use anyhow::Result;
use async_trait::async_trait;
use function_name::named;
use itertools::Itertools;
use mmb_core::exchanges::common::{
    ActivePosition, ClosedPosition, CurrencyPair, ExchangeError, ExchangeErrorType, Price,
};
use mmb_core::exchanges::events::ExchangeBalancesAndPositions;
use mmb_core::exchanges::general::exchange::RequestResult;
use mmb_core::exchanges::general::order::cancel::CancelOrderResult;
use mmb_core::exchanges::general::order::create::CreateOrderResult;
use mmb_core::exchanges::general::order::get_order_trades::OrderTrade;
use mmb_core::exchanges::general::symbol::Symbol;
use mmb_core::exchanges::rest_client;
use mmb_core::exchanges::traits::{ExchangeClient, Support};
use mmb_core::orders::fill::EventSourceType;
use mmb_core::orders::order::*;
use mmb_core::orders::pool::OrderRef;
use mmb_utils::DateTime;
use std::sync::Arc;

#[async_trait]
impl ExchangeClient for Binance {
    async fn create_order(&self, order: &OrderRef) -> CreateOrderResult {
        match self.request_create_order(order).await {
            Ok(request_outcome) => match self.get_order_id(&request_outcome) {
                Ok(order_id) => CreateOrderResult::succeed(&order_id, EventSourceType::Rest),
                Err(error) => CreateOrderResult::failed(error, EventSourceType::Rest),
            },
            Err(err) => CreateOrderResult::failed(err, EventSourceType::Rest),
        }
    }

    async fn cancel_order(&self, order: OrderCancelling) -> CancelOrderResult {
        let order_header = order.header.clone();

        match self.request_cancel_order(order).await {
            Ok(_) => CancelOrderResult::succeed(
                order_header.client_order_id.clone(),
                EventSourceType::Rest,
                None,
            ),
            Err(err) => CancelOrderResult::failed(err, EventSourceType::Rest),
        }
    }

    #[named]
    async fn cancel_all_orders(&self, currency_pair: CurrencyPair) -> Result<()> {
        let specific_currency_pair = self.get_specific_currency_pair(currency_pair);

        let host = &self.hosts.rest_host;
        let path_to_delete = "/api/v3/openOrders";

        let mut http_params = vec![(
            "symbol".to_owned(),
            specific_currency_pair.as_str().to_owned(),
        )];
        self.add_authentification_headers(&mut http_params)?;

        let full_url = rest_client::build_uri(host, path_to_delete, &http_params);

        self.rest_client
            .delete(
                full_url,
                &self.settings.api_key,
                function_name!(),
                "".to_string(),
            )
            .await?;

        Ok(())
    }

    async fn get_open_orders(&self) -> Result<Vec<OrderInfo>> {
        let response = self.request_open_orders().await?;

        Ok(self.parse_open_orders(&response))
    }

    async fn get_open_orders_by_currency_pair(
        &self,
        currency_pair: CurrencyPair,
    ) -> Result<Vec<OrderInfo>> {
        let response = self
            .request_open_orders_by_currency_pair(currency_pair)
            .await?;

        Ok(self.parse_open_orders(&response))
    }

    async fn get_order_info(&self, order: &OrderRef) -> Result<OrderInfo, ExchangeError> {
        match self.request_order_info(order).await {
            Ok(request_outcome) => Ok(self.parse_order_info(&request_outcome)),
            Err(error) => Err(ExchangeError::new(
                ExchangeErrorType::ParsingError,
                error.to_string(),
                None,
            )),
        }
    }

    async fn close_position(
        &self,
        position: &ActivePosition,
        price: Option<Price>,
    ) -> Result<ClosedPosition> {
        let response = self.request_close_position(position, price).await?;
        let binance_order: BinanceOrderInfo = serde_json::from_str(&response.content)
            .expect("Unable to parse response content for get_open_orders request");

        Ok(ClosedPosition::new(
            ExchangeOrderId::from(binance_order.exchange_order_id.to_string().as_ref()),
            binance_order.orig_quantity,
        ))
    }

    async fn get_active_positions(&self) -> Result<Vec<ActivePosition>> {
        let response = self.request_get_position().await?;
        let binance_positions: Vec<BinancePosition> = serde_json::from_str(&response.content)
            .expect("Unable to parse response content for get_active_positions_core request");

        Ok(binance_positions
            .into_iter()
            .map(|x| self.binance_position_to_active_position(x))
            .collect_vec())
    }

    async fn get_balance(&self, is_spot: bool) -> Result<ExchangeBalancesAndPositions> {
        let response = match is_spot {
            true => self.request_get_balance_spot().await?,
            false => self.request_get_balance().await?,
        };

        Ok(self.parse_get_balance(&response))
    }

    async fn get_my_trades(
        &self,
        symbol: &Symbol,
        last_date_time: Option<DateTime>,
    ) -> Result<RequestResult<Vec<OrderTrade>>> {
        // TODO Add metric UseTimeMetric(RequestType::GetMyTrades)
        match self.request_my_trades(symbol, last_date_time).await {
            Ok(response) => match self.parse_get_my_trades(&response, last_date_time) {
                Ok(data) => Ok(RequestResult::Success(data)),
                Err(_) => Ok(RequestResult::Error(ExchangeError::unknown(
                    &response.content,
                ))),
            },
            Err(error) => Ok(RequestResult::Error(ExchangeError::new(
                ExchangeErrorType::ParsingError,
                error.to_string(),
                None,
            ))),
        }
    }

    async fn build_all_symbols(&self) -> Result<Vec<Arc<Symbol>>> {
        let response = &self.request_all_symbols().await?;

        self.parse_all_symbols(response)
    }
}
