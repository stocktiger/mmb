use crate::services::liquidity::{
    Amount, LiquidityData, LiquidityOrderSide, Price, TransactionOrderSide, TransactionTradeSide,
};
use actix::prelude::*;
use itertools::Itertools;
use rust_decimal::prelude::Zero;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Serialize, Deserialize, Message, Clone)]
#[rtype(result = "()")]
#[serde(rename_all = "camelCase")]
pub struct LiquidityResponseBody {
    pub orders_state_and_transactions: OrderStateAndTransactions,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OrderStateAndTransactions {
    pub exchange_name: String,
    pub currency_code_pair: String,
    pub desired_amount: Amount,
    pub sell: Orders,
    pub buy: Orders,
    pub transactions: Vec<Transaction>,
    pub indicators: Indicators,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Orders {
    pub orders: Vec<Order>,
    pub snapshot: Vec<(Price, Amount)>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Order {
    pub amount: Amount,
    pub price: Price,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    pub id: String,
    pub date_time: String,
    pub price: Price,
    pub amount: Amount,
    pub hedged: Option<String>,
    pub profit_loss_pct: Option<String>,
    pub status: String,
    pub trades: Vec<Trade>,
    pub side: TransactionOrderSide,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Trade {
    pub exchange_name: String,
    pub date_time: String,
    pub price: Price,
    pub amount: Amount,
    pub exchange_order_id: String,
    pub side: Option<TransactionTradeSide>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Indicators {
    pub volume_pct: Decimal,
    pub bid_pct: Decimal,
    pub ask_pct: Decimal,
    pub spread: Option<Decimal>,
    pub total_volume: Option<Amount>,
    pub total_bid: Option<Amount>,
    pub total_ask: Option<Amount>,
}

impl From<LiquidityData> for LiquidityResponseBody {
    fn from(liquidity_data: LiquidityData) -> Self {
        let sell_snapshot = liquidity_data
            .order_book
            .snapshot
            .asks
            .iter()
            .map(|price_level| (price_level.price, price_level.amount))
            .collect_vec();
        let buy_snapshot = liquidity_data
            .order_book
            .snapshot
            .bids
            .iter()
            .map(|price_level| (price_level.price, price_level.amount))
            .collect_vec();

        let mut buy_orders: Vec<Order> = vec![];
        let mut sell_orders: Vec<Order> = vec![];

        liquidity_data
            .order_book
            .orders
            .iter()
            .for_each(|order| match order.side {
                LiquidityOrderSide::Buy => buy_orders.push(Order {
                    amount: order.amount,
                    price: order.price,
                }),
                LiquidityOrderSide::Sell => sell_orders.push(Order {
                    amount: order.amount,
                    price: order.price,
                }),
            });

        let indicators = get_indicators(
            &liquidity_data,
            &buy_snapshot,
            &sell_snapshot,
            liquidity_data.desired_amount,
        );

        let transactions = liquidity_data
            .transactions
            .into_iter()
            .map(|t| {
                let trades = t
                    .trades
                    .into_iter()
                    .map(|tr| Trade {
                        exchange_name: tr.exchange_id,
                        date_time: t.transaction_creation_time.clone(),
                        price: tr.price,
                        amount: tr.amount,
                        exchange_order_id: tr.exchange_order_id,
                        side: tr.side,
                    })
                    .collect_vec();
                Transaction {
                    id: t.transaction_id,
                    date_time: t.transaction_creation_time,
                    price: t.price,
                    amount: t.amount,
                    hedged: t.hedged,
                    profit_loss_pct: t.profit_loss_pct,
                    status: t.status,
                    trades,
                    side: t.side,
                }
            })
            .collect_vec();

        let state = OrderStateAndTransactions {
            exchange_name: liquidity_data.order_book.exchange_id,
            currency_code_pair: liquidity_data.order_book.currency_pair,
            desired_amount: liquidity_data.desired_amount,
            sell: Orders {
                orders: sell_orders,
                snapshot: sell_snapshot,
            },
            buy: Orders {
                orders: buy_orders,
                snapshot: buy_snapshot,
            },
            transactions,
            indicators,
        };

        Self {
            orders_state_and_transactions: state,
        }
    }
}

fn get_indicators(
    liquidity_data: &LiquidityData,
    buy_snapshot: &[(Price, Amount)],
    sell_snapshot: &[(Price, Amount)],
    desired_amount: Amount,
) -> Indicators {
    fn get_total(data: &LiquidityData, side: LiquidityOrderSide) -> Option<Amount> {
        let mut iter = data
            .order_book
            .orders
            .iter()
            .filter(|it| it.side == side)
            .map(|it| it.remaining_amount);

        iter.next()
            .map(|first_amount| first_amount + iter.sum::<Amount>())
    }

    fn cmp_prices(a: &Price, b: &Price) -> Ordering {
        a.partial_cmp(b)
            .unwrap_or_else(|| panic!("Error partial_cmp {} {}", a, b))
    }

    let top_bid_price = buy_snapshot.iter().map(|x| x.0).max_by(cmp_prices);
    let top_ask_price = sell_snapshot.iter().map(|x| x.0).min_by(cmp_prices);

    let total_bid = get_total(liquidity_data, LiquidityOrderSide::Buy);
    let total_ask = get_total(liquidity_data, LiquidityOrderSide::Sell);

    let (spread, total_volume, volume_pct, bid_pct, ask_pct) = calc_indicators(
        top_bid_price,
        top_ask_price,
        total_bid,
        total_ask,
        desired_amount,
    );

    Indicators {
        volume_pct,
        bid_pct,
        ask_pct,
        spread,
        total_volume,
        total_bid,
        total_ask,
    }
}

fn calc_indicators(
    top_bid_price: Option<Price>,
    top_ask_price: Option<Price>,
    total_bid: Option<Amount>,
    total_ask: Option<Amount>,
    desired_amount: Amount,
) -> (Option<Decimal>, Option<Decimal>, Decimal, Decimal, Decimal) {
    let spread = match (top_ask_price, top_bid_price) {
        (Some(ask), Some(bid)) => Some((ask - bid) / ask * dec!(100)),
        _ => None,
    };

    let total_volume = match (total_ask, total_bid) {
        (Some(ask), Some(bid)) => Some(ask + bid),
        _ => None,
    };

    let bid_pct = match (total_bid, desired_amount.is_zero()) {
        (Some(value), false) => value / desired_amount * dec!(100),
        _ => Decimal::zero(),
    };

    let ask_pct = match (total_ask, desired_amount.is_zero()) {
        (Some(value), false) => value / desired_amount * dec!(100),
        _ => Decimal::zero(),
    };

    // total_volume should be less or equal than desired_amount
    let volume_pct = match (total_volume, desired_amount.is_zero()) {
        (Some(value), false) => value / desired_amount * dec!(100),
        _ => Decimal::zero(),
    };

    (spread, total_volume, volume_pct, bid_pct, ask_pct)
}

#[cfg(test)]
mod tests {
    use crate::ws::commands::liquidity::calc_indicators;
    use rust_decimal::prelude::Zero;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    #[test]
    fn calc_indicators_test_values() {
        let (spread, total_volume, volume_pct, bid_pct, ask_pct) = calc_indicators(
            Some(dec!(1)),
            Some(dec!(2)),
            Some(dec!(0.0008)),
            Some(dec!(0.0002)),
            dec!(0.001),
        );
        assert_eq!(spread, Some(dec!(50)));
        assert_eq!(total_volume, Some(dec!(0.001)));
        assert_eq!(bid_pct, dec!(80));
        assert_eq!(ask_pct, dec!(20));
        assert_eq!(volume_pct, dec!(100));
    }

    #[test]
    fn calc_indicators_test_none() {
        let (spread, total_volume, volume_pct, bid_pct, ask_pct) =
            calc_indicators(None, None, None, None, dec!(0.001));
        assert_eq!(spread, None);
        assert_eq!(total_volume, None);
        assert_eq!(bid_pct, Decimal::zero());
        assert_eq!(ask_pct, Decimal::zero());
        assert_eq!(volume_pct, Decimal::zero());
    }
}
