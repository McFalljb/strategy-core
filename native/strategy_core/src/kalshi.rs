use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, de};

use crate::models::JsonValue;

pub type KalshiFixedPrice = String;
pub type KalshiFixedCount = String;
pub type KalshiMarketSide = String;
pub type KalshiMarketResult = String;
pub type KalshiOrderAction = String;
pub type KalshiOrderType = String;
pub type KalshiOrderStatus = String;
pub type KalshiTimeInForce = String;
pub type KalshiImmediateTimeInForce = String;
pub type KalshiSelfTradePreventionType = String;
pub type KalshiMarketStatus = String;
pub type KalshiSubscriptionUpdateAction = String;
pub type KalshiWsChannel = String;
pub type KalshiMarketLifecycleEventType = String;
pub type KalshiPriceLevelStructure = String;
pub type KalshiCollateralReturnType = String;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KalshiOrderCreateRequest {
    pub ticker: String,
    pub side: String,
    pub action: String,
    pub client_order_id: Option<String>,
    pub count: Option<i64>,
    pub count_fp: Option<KalshiFixedCount>,
    pub yes_price: Option<i64>,
    pub no_price: Option<i64>,
    pub yes_price_dollars: Option<KalshiFixedPrice>,
    pub no_price_dollars: Option<KalshiFixedPrice>,
    pub expiration_ts: Option<i64>,
    pub time_in_force: Option<String>,
    pub buy_max_cost: Option<i64>,
    pub post_only: Option<bool>,
    pub reduce_only: Option<bool>,
    pub sell_position_floor: Option<i64>,
    pub self_trade_prevention_type: Option<String>,
    pub order_group_id: Option<String>,
    pub cancel_order_on_pause: Option<bool>,
    #[serde(default)]
    pub subaccount: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KalshiOrder {
    #[serde(default)]
    pub order_id: String,
    #[serde(default)]
    pub user_id: String,
    pub client_order_id: Option<String>,
    #[serde(default)]
    pub ticker: String,
    #[serde(default = "default_side_yes")]
    pub side: String,
    #[serde(default = "default_action_buy")]
    pub action: String,
    #[serde(rename = "type", default = "default_order_type")]
    pub order_type: String,
    #[serde(default = "default_order_status")]
    pub status: String,
    pub yes_price_dollars: Option<KalshiFixedPrice>,
    pub no_price_dollars: Option<KalshiFixedPrice>,
    pub fill_count_fp: Option<KalshiFixedCount>,
    pub remaining_count_fp: Option<KalshiFixedCount>,
    pub initial_count_fp: Option<KalshiFixedCount>,
    pub taker_fill_cost_dollars: Option<KalshiFixedPrice>,
    pub maker_fill_cost_dollars: Option<KalshiFixedPrice>,
    pub taker_fees_dollars: Option<KalshiFixedPrice>,
    pub maker_fees_dollars: Option<KalshiFixedPrice>,
    pub expiration_time: Option<DateTime<Utc>>,
    pub created_time: Option<DateTime<Utc>>,
    pub last_update_time: Option<DateTime<Utc>>,
    pub self_trade_prevention_type: Option<String>,
    pub order_group_id: Option<String>,
    pub cancel_order_on_pause: Option<bool>,
    pub subaccount_number: Option<i64>,
}

impl Default for KalshiOrder {
    fn default() -> Self {
        Self {
            order_id: String::new(),
            user_id: String::new(),
            client_order_id: None,
            ticker: String::new(),
            side: default_side_yes(),
            action: default_action_buy(),
            order_type: default_order_type(),
            status: default_order_status(),
            yes_price_dollars: None,
            no_price_dollars: None,
            fill_count_fp: None,
            remaining_count_fp: None,
            initial_count_fp: None,
            taker_fill_cost_dollars: None,
            maker_fill_cost_dollars: None,
            taker_fees_dollars: None,
            maker_fees_dollars: None,
            expiration_time: None,
            created_time: None,
            last_update_time: None,
            self_trade_prevention_type: None,
            order_group_id: None,
            cancel_order_on_pause: None,
            subaccount_number: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct KalshiCreateOrderResponse {
    pub order: Option<KalshiOrder>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct KalshiGetOrderResponse {
    pub order: Option<KalshiOrder>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct KalshiGetOrdersResponse {
    #[serde(default)]
    pub orders: Vec<KalshiOrder>,
    pub cursor: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KalshiOrderbookLevel {
    pub price_dollars: KalshiFixedPrice,
    pub count_fp: KalshiFixedCount,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct KalshiOrderbook {
    #[serde(default)]
    pub yes_dollars: Vec<KalshiOrderbookLevel>,
    #[serde(default)]
    pub no_dollars: Vec<KalshiOrderbookLevel>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KalshiMarketOrderbook {
    pub ticker: String,
    pub orderbook_fp: KalshiOrderbook,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KalshiGetOrderbookResponse {
    pub orderbook_fp: KalshiOrderbook,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct KalshiGetOrderbooksResponse {
    #[serde(default)]
    pub orderbooks: Vec<KalshiMarketOrderbook>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KalshiPriceRange {
    pub start: KalshiFixedPrice,
    pub end: KalshiFixedPrice,
    pub step: KalshiFixedPrice,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct KalshiMveSelectedLeg {
    #[serde(default)]
    pub event_ticker: String,
    #[serde(default)]
    pub market_ticker: String,
    #[serde(default)]
    pub side: String,
    pub yes_settlement_value_dollars: Option<KalshiFixedPrice>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KalshiMarket {
    #[serde(default)]
    pub ticker: String,
    #[serde(default)]
    pub event_ticker: String,
    #[serde(default = "default_market_type")]
    pub market_type: String,
    #[serde(default = "default_market_status")]
    pub status: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub yes_sub_title: String,
    #[serde(default)]
    pub no_sub_title: String,
    pub created_time: Option<DateTime<Utc>>,
    pub updated_time: Option<DateTime<Utc>>,
    pub open_time: Option<DateTime<Utc>>,
    pub close_time: Option<DateTime<Utc>>,
    pub latest_expiration_time: Option<DateTime<Utc>>,
    pub expected_expiration_time: Option<DateTime<Utc>>,
    pub expiration_time: Option<DateTime<Utc>>,
    pub settlement_timer_seconds: Option<i64>,
    pub result: Option<String>,
    pub can_close_early: Option<bool>,
    pub fractional_trading_enabled: Option<bool>,
    pub yes_bid_dollars: Option<KalshiFixedPrice>,
    pub yes_bid_size_fp: Option<KalshiFixedCount>,
    pub yes_ask_dollars: Option<KalshiFixedPrice>,
    pub yes_ask_size_fp: Option<KalshiFixedCount>,
    pub no_bid_dollars: Option<KalshiFixedPrice>,
    pub no_ask_dollars: Option<KalshiFixedPrice>,
    pub last_price_dollars: Option<KalshiFixedPrice>,
    pub volume_fp: Option<KalshiFixedCount>,
    pub volume_24h_fp: Option<KalshiFixedCount>,
    pub open_interest_fp: Option<KalshiFixedCount>,
    pub dollar_volume: Option<i64>,
    pub dollar_open_interest: Option<i64>,
    pub notional_value_dollars: Option<KalshiFixedPrice>,
    pub liquidity_dollars: Option<KalshiFixedPrice>,
    pub previous_yes_bid_dollars: Option<KalshiFixedPrice>,
    pub previous_yes_ask_dollars: Option<KalshiFixedPrice>,
    pub previous_price_dollars: Option<KalshiFixedPrice>,
    pub expiration_value: Option<String>,
    pub rules_primary: Option<String>,
    pub rules_secondary: Option<String>,
    pub response_price_units: Option<String>,
    pub settlement_value_dollars: Option<KalshiFixedPrice>,
    pub settlement_ts: Option<DateTime<Utc>>,
    pub fee_waiver_expiration_time: Option<DateTime<Utc>>,
    pub early_close_condition: Option<String>,
    pub price_level_structure: Option<String>,
    #[serde(default)]
    pub price_ranges: Vec<KalshiPriceRange>,
    pub tick_size: Option<i64>,
    pub strike_type: Option<String>,
    pub floor_strike: Option<i64>,
    pub cap_strike: Option<i64>,
    pub functional_strike: Option<String>,
    #[serde(default)]
    pub custom_strike: BTreeMap<String, JsonValue>,
    pub mve_collection_ticker: Option<String>,
    #[serde(default)]
    pub mve_selected_legs: Vec<KalshiMveSelectedLeg>,
    pub primary_participant_key: Option<String>,
    pub is_provisional: Option<bool>,
}

impl Default for KalshiMarket {
    fn default() -> Self {
        Self {
            ticker: String::new(),
            event_ticker: String::new(),
            market_type: default_market_type(),
            status: default_market_status(),
            title: String::new(),
            subtitle: String::new(),
            yes_sub_title: String::new(),
            no_sub_title: String::new(),
            created_time: None,
            updated_time: None,
            open_time: None,
            close_time: None,
            latest_expiration_time: None,
            expected_expiration_time: None,
            expiration_time: None,
            settlement_timer_seconds: None,
            result: None,
            can_close_early: None,
            fractional_trading_enabled: None,
            yes_bid_dollars: None,
            yes_bid_size_fp: None,
            yes_ask_dollars: None,
            yes_ask_size_fp: None,
            no_bid_dollars: None,
            no_ask_dollars: None,
            last_price_dollars: None,
            volume_fp: None,
            volume_24h_fp: None,
            open_interest_fp: None,
            dollar_volume: None,
            dollar_open_interest: None,
            notional_value_dollars: None,
            liquidity_dollars: None,
            previous_yes_bid_dollars: None,
            previous_yes_ask_dollars: None,
            previous_price_dollars: None,
            expiration_value: None,
            rules_primary: None,
            rules_secondary: None,
            response_price_units: None,
            settlement_value_dollars: None,
            settlement_ts: None,
            fee_waiver_expiration_time: None,
            early_close_condition: None,
            price_level_structure: None,
            price_ranges: Vec::new(),
            tick_size: None,
            strike_type: None,
            floor_strike: None,
            cap_strike: None,
            functional_strike: None,
            custom_strike: BTreeMap::new(),
            mve_collection_ticker: None,
            mve_selected_legs: Vec::new(),
            primary_participant_key: None,
            is_provisional: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct KalshiGetMarketResponse {
    pub market: Option<KalshiMarket>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct KalshiMarketsPage {
    #[serde(default)]
    pub markets: Vec<KalshiMarket>,
    pub cursor: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KalshiSubscribeCommand {
    pub id: i64,
    pub channels: Vec<String>,
    pub market_ticker: Option<String>,
    #[serde(default)]
    pub market_tickers: Vec<String>,
    pub market_id: Option<String>,
    #[serde(default)]
    pub market_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KalshiUnsubscribeCommand {
    pub id: i64,
    pub sids: Vec<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KalshiListSubscriptionsCommand {
    pub id: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KalshiUpdateSubscriptionCommand {
    pub id: i64,
    pub action: String,
    pub market_tickers: Vec<String>,
    pub sid: Option<i64>,
    #[serde(default)]
    pub sids: Vec<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KalshiOrderbookSnapshotMessage {
    pub sid: i64,
    pub seq: i64,
    pub market_ticker: String,
    pub market_id: String,
    #[serde(default)]
    pub yes_dollars_fp: Vec<KalshiOrderbookLevel>,
    #[serde(default)]
    pub no_dollars_fp: Vec<KalshiOrderbookLevel>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KalshiOrderbookDeltaMessage {
    pub sid: i64,
    pub seq: i64,
    pub market_ticker: String,
    pub market_id: String,
    pub price_dollars: KalshiFixedPrice,
    pub delta_fp: KalshiFixedCount,
    pub side: String,
    pub client_order_id: Option<String>,
    pub subaccount: Option<i64>,
    pub ts: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KalshiTickerMessage {
    pub sid: i64,
    pub market_ticker: String,
    pub market_id: String,
    pub price_dollars: Option<KalshiFixedPrice>,
    pub yes_bid_dollars: Option<KalshiFixedPrice>,
    pub yes_ask_dollars: Option<KalshiFixedPrice>,
    pub volume_fp: Option<KalshiFixedCount>,
    pub open_interest_fp: Option<KalshiFixedCount>,
    pub dollar_volume: Option<i64>,
    pub dollar_open_interest: Option<i64>,
    pub yes_bid_size_fp: Option<KalshiFixedCount>,
    pub yes_ask_size_fp: Option<KalshiFixedCount>,
    pub last_trade_size_fp: Option<KalshiFixedCount>,
    pub ts: Option<i64>,
    pub time: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KalshiTradeMessage {
    pub sid: i64,
    pub trade_id: String,
    pub market_ticker: String,
    pub yes_price_dollars: Option<KalshiFixedPrice>,
    pub no_price_dollars: Option<KalshiFixedPrice>,
    pub count_fp: Option<KalshiFixedCount>,
    pub taker_side: Option<String>,
    pub ts: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KalshiUserOrderMessage {
    pub sid: i64,
    pub order_id: String,
    pub user_id: String,
    pub ticker: String,
    pub status: String,
    pub side: String,
    pub is_yes: bool,
    pub yes_price_dollars: Option<KalshiFixedPrice>,
    pub fill_count_fp: Option<KalshiFixedCount>,
    pub remaining_count_fp: Option<KalshiFixedCount>,
    pub initial_count_fp: Option<KalshiFixedCount>,
    pub taker_fill_cost_dollars: Option<KalshiFixedPrice>,
    pub maker_fill_cost_dollars: Option<KalshiFixedPrice>,
    pub client_order_id: Option<String>,
    pub order_group_id: Option<String>,
    pub self_trade_prevention_type: Option<String>,
    pub created_time: Option<DateTime<Utc>>,
    pub expiration_time: Option<DateTime<Utc>>,
    pub subaccount_number: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KalshiUserFillMessage {
    pub sid: i64,
    pub trade_id: String,
    pub order_id: String,
    pub market_ticker: String,
    pub is_taker: bool,
    pub side: String,
    pub yes_price_dollars: Option<KalshiFixedPrice>,
    pub count_fp: Option<KalshiFixedCount>,
    pub fee_cost: Option<KalshiFixedPrice>,
    #[serde(default = "default_action_buy")]
    pub action: String,
    pub ts: Option<i64>,
    pub client_order_id: Option<String>,
    pub post_position_fp: Option<KalshiFixedCount>,
    pub purchased_side: Option<String>,
    pub subaccount: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KalshiMarketPositionMessage {
    pub sid: i64,
    pub user_id: String,
    pub market_ticker: String,
    pub position_fp: Option<KalshiFixedCount>,
    pub position_cost_dollars: Option<KalshiFixedPrice>,
    pub realized_pnl_dollars: Option<KalshiFixedPrice>,
    pub fees_paid_dollars: Option<KalshiFixedPrice>,
    pub position_fee_cost_dollars: Option<KalshiFixedPrice>,
    pub volume_fp: Option<KalshiFixedCount>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KalshiMarketLifecycleMetadata {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub yes_sub_title: String,
    #[serde(default)]
    pub no_sub_title: String,
    #[serde(default)]
    pub rules_primary: String,
    #[serde(default)]
    pub rules_secondary: String,
    pub can_close_early: Option<bool>,
    #[serde(default)]
    pub event_ticker: String,
    pub expected_expiration_ts: Option<i64>,
    pub strike_type: Option<String>,
    pub floor_strike: Option<f64>,
    pub cap_strike: Option<f64>,
    #[serde(default)]
    pub custom_strike: BTreeMap<String, JsonValue>,
}

impl Default for KalshiMarketLifecycleMetadata {
    fn default() -> Self {
        Self {
            name: String::new(),
            title: String::new(),
            yes_sub_title: String::new(),
            no_sub_title: String::new(),
            rules_primary: String::new(),
            rules_secondary: String::new(),
            can_close_early: None,
            event_ticker: String::new(),
            expected_expiration_ts: None,
            strike_type: None,
            floor_strike: None,
            cap_strike: None,
            custom_strike: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KalshiMarketLifecycleMessage {
    pub sid: i64,
    pub event_type: String,
    pub market_ticker: String,
    pub open_ts: Option<i64>,
    pub close_ts: Option<i64>,
    pub result: Option<String>,
    pub determination_ts: Option<i64>,
    pub settlement_value: Option<KalshiFixedPrice>,
    pub settled_ts: Option<i64>,
    pub is_deactivated: Option<bool>,
    pub fractional_trading_enabled: Option<bool>,
    pub price_level_structure: Option<String>,
    pub additional_metadata: Option<KalshiMarketLifecycleMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KalshiEventLifecycleMessage {
    pub sid: i64,
    pub event_ticker: String,
    pub title: String,
    pub subtitle: String,
    #[serde(default)]
    pub collateral_return_type: String,
    #[serde(default)]
    pub series_ticker: String,
    pub strike_date: Option<i64>,
    pub strike_period: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum KalshiWsMessage {
    OrderbookSnapshot(KalshiOrderbookSnapshotMessage),
    OrderbookDelta(KalshiOrderbookDeltaMessage),
    Ticker(KalshiTickerMessage),
    Trade(KalshiTradeMessage),
    UserOrder(KalshiUserOrderMessage),
    UserFill(KalshiUserFillMessage),
    MarketPosition(KalshiMarketPositionMessage),
    MarketLifecycle(KalshiMarketLifecycleMessage),
    EventLifecycle(KalshiEventLifecycleMessage),
}

impl<'de> Deserialize<'de> for KalshiWsMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| de::Error::custom("Kalshi websocket message must be an object"))?;

        if object.contains_key("price_dollars")
            && object.contains_key("delta_fp")
            && object.contains_key("side")
            && object.contains_key("seq")
        {
            return decode_ws_message(value).map(Self::OrderbookDelta);
        }
        if object.contains_key("seq") && object.contains_key("market_id") {
            return decode_ws_message(value).map(Self::OrderbookSnapshot);
        }
        if object.contains_key("order_id")
            && object.contains_key("trade_id")
            && object.contains_key("market_ticker")
        {
            return decode_ws_message(value).map(Self::UserFill);
        }
        if object.contains_key("order_id") && object.contains_key("ticker") {
            return decode_ws_message(value).map(Self::UserOrder);
        }
        if object.contains_key("trade_id") && object.contains_key("market_ticker") {
            return decode_ws_message(value).map(Self::Trade);
        }
        if object.contains_key("user_id") && object.contains_key("market_ticker") {
            return decode_ws_message(value).map(Self::MarketPosition);
        }
        if object.contains_key("event_type") && object.contains_key("market_ticker") {
            return decode_ws_message(value).map(Self::MarketLifecycle);
        }
        if object.contains_key("event_ticker") {
            return decode_ws_message(value).map(Self::EventLifecycle);
        }
        if object.contains_key("market_ticker") && object.contains_key("market_id") {
            return decode_ws_message(value).map(Self::Ticker);
        }

        Err(de::Error::custom(
            "unrecognized Kalshi websocket message shape",
        ))
    }
}

fn decode_ws_message<T, E>(value: serde_json::Value) -> Result<T, E>
where
    T: for<'de> Deserialize<'de>,
    E: de::Error,
{
    serde_json::from_value(value).map_err(E::custom)
}

fn default_side_yes() -> String {
    "yes".to_string()
}

fn default_action_buy() -> String {
    "buy".to_string()
}

fn default_order_type() -> String {
    "limit".to_string()
}

fn default_order_status() -> String {
    "resting".to_string()
}

fn default_market_type() -> String {
    "binary".to_string()
}

fn default_market_status() -> String {
    "open".to_string()
}
