use std::collections::HashMap;

use crate::balance::manager::{
    balance_position_by_fill_amount::BalancePositionByFillAmount,
    balance_reservation::BalanceReservation,
};
use crate::exchanges::common::CurrencyCode;
use crate::exchanges::common::ExchangeAccountId;
use crate::exchanges::common::MarketAccountId;
use crate::misc::service_value_tree::ServiceValueTree;
use crate::orders::fill::OrderFill;
use crate::orders::order::ReservationId;

use mmb_utils::DateTime;
use rust_decimal::Decimal;

pub struct Balances {
    pub version: usize,
    pub init_time: DateTime,
    pub balances_by_exchange_id: Option<HashMap<ExchangeAccountId, HashMap<CurrencyCode, Decimal>>>,
    pub virtual_diff_balances: Option<ServiceValueTree>,

    /// In Amount currency
    pub reserved_amount: Option<ServiceValueTree>,

    /// In Amount currency
    pub position_by_fill_amount: Option<BalancePositionByFillAmount>,

    /// In Amount currency
    pub amount_limits: Option<ServiceValueTree>,
    pub balance_reservations_by_reservation_id: Option<HashMap<ReservationId, BalanceReservation>>,

    pub last_order_fills: HashMap<MarketAccountId, OrderFill>,
}

impl Balances {
    pub fn new(
        balances_by_exchange_id: HashMap<ExchangeAccountId, HashMap<CurrencyCode, Decimal>>,
        init_time: DateTime,
        virtual_diff_balances: ServiceValueTree,
        reserved_amount: ServiceValueTree,
        position_by_fill_amount: BalancePositionByFillAmount,
        amount_limits: ServiceValueTree,
        balance_reservations_by_reservation_id: HashMap<ReservationId, BalanceReservation>,
    ) -> Self {
        Self {
            version: Balances::get_current_version(),
            init_time,
            balances_by_exchange_id: Some(balances_by_exchange_id),
            virtual_diff_balances: Some(virtual_diff_balances),
            reserved_amount: Some(reserved_amount),
            position_by_fill_amount: Some(position_by_fill_amount),
            amount_limits: Some(amount_limits),
            balance_reservations_by_reservation_id: Some(balance_reservations_by_reservation_id),
            last_order_fills: HashMap::new(),
        }
    }

    pub fn get_current_version() -> usize {
        1
    }
}
