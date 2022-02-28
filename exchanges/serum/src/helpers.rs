use mmb_core::orders::order::OrderSide;
use serum_dex::matching::Side;
use solana_program::pubkey::Pubkey;

pub trait ToOrderSide {
    fn to_order_side(&self) -> OrderSide;
}

impl ToOrderSide for Side {
    fn to_order_side(&self) -> OrderSide {
        match self {
            Side::Bid => OrderSide::Buy,
            Side::Ask => OrderSide::Sell,
        }
    }
}

pub trait ToSerumSide {
    fn to_serum_side(&self) -> Side;
}

impl ToSerumSide for OrderSide {
    fn to_serum_side(&self) -> Side {
        match self {
            OrderSide::Buy => Side::Bid,
            OrderSide::Sell => Side::Ask,
        }
    }
}

pub trait FromU64Array {
    fn from_u64_array(arr: [u64; 4]) -> Self;
}

impl FromU64Array for Pubkey {
    fn from_u64_array(arr: [u64; 4]) -> Self {
        let mut key: [u8; 32] = [0; 32];
        arr.iter()
            .flat_map(|x| x.to_le_bytes())
            .enumerate()
            .for_each(|(i, x)| key[i] = x);

        Pubkey::new_from_array(key)
    }
}