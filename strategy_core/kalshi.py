"""Kalshi REST and WebSocket payload models informed by the public exchange docs.

This module intentionally captures typed request/response and realtime payload
shapes, not HTTP clients, websocket subscribers, auth, or retry behavior.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from types import MappingProxyType
from typing import TYPE_CHECKING, Literal

if TYPE_CHECKING:
    from collections.abc import Iterable, Mapping
    from datetime import datetime

KalshiFixedPrice = str
KalshiFixedCount = str
KalshiMarketSide = Literal["yes", "no"]
KalshiMarketResult = Literal["yes", "no", "scalar", ""]
KalshiOrderAction = Literal["buy", "sell"]
KalshiOrderType = Literal["limit"]
KalshiOrderStatus = Literal["resting", "canceled", "executed"]
KalshiTimeInForce = Literal["fill_or_kill", "good_till_canceled", "immediate_or_cancel"]
KalshiImmediateTimeInForce = Literal["fill_or_kill", "immediate_or_cancel"]
KalshiSelfTradePreventionType = Literal["taker_at_cross", "maker"]
KalshiMarketStatus = Literal["unopened", "open", "paused", "closed", "settled"]
KalshiSubscriptionUpdateAction = Literal["add_markets", "delete_markets"]
KalshiWsChannel = Literal[
    "orderbook_delta",
    "ticker",
    "trade",
    "fill",
    "market_positions",
    "market_lifecycle_v2",
    "multivariate_market_lifecycle",
    "multivariate",
    "communications",
    "order_group_updates",
    "user_orders",
]
KalshiMarketLifecycleEventType = Literal[
    "created",
    "deactivated",
    "activated",
    "close_date_updated",
    "determined",
    "settled",
    "fractional_trading_updated",
    "price_level_structure_updated",
]
KalshiPriceLevelStructure = Literal["linear_cent", "deci_cent", "tapered_deci_cent"]
KalshiCollateralReturnType = Literal["MECNET", "DIRECNET", ""]


def _freeze_tuple[T](values: Iterable[T]) -> tuple[T, ...]:
    return tuple(values)


def _freeze_mapping[K, V](values: Mapping[K, V]) -> Mapping[K, V]:
    return MappingProxyType(dict(values))


@dataclass(frozen=True, slots=True)
class KalshiOrderCreateRequest:
    """REST create-order payload.

    Kalshi currently models immediate execution through ``time_in_force`` and
    ``buy_max_cost`` rather than a separate market-order type.
    """

    ticker: str
    side: KalshiMarketSide | str
    action: KalshiOrderAction | str
    client_order_id: str | None = None
    count: int | None = None
    count_fp: KalshiFixedCount | None = None
    yes_price: int | None = None
    no_price: int | None = None
    yes_price_dollars: KalshiFixedPrice | None = None
    no_price_dollars: KalshiFixedPrice | None = None
    expiration_ts: int | None = None
    time_in_force: KalshiTimeInForce | str | None = None
    buy_max_cost: int | None = None
    post_only: bool | None = None
    reduce_only: bool | None = None
    sell_position_floor: int | None = None
    self_trade_prevention_type: KalshiSelfTradePreventionType | str | None = None
    order_group_id: str | None = None
    cancel_order_on_pause: bool | None = None
    subaccount: int = 0


@dataclass(frozen=True, slots=True)
class KalshiOrder:
    """REST order payload returned by create/get/list endpoints."""

    order_id: str = ""
    user_id: str = ""
    client_order_id: str | None = None
    ticker: str = ""
    side: KalshiMarketSide | str = "yes"
    action: KalshiOrderAction | str = "buy"
    type: KalshiOrderType | str = "limit"
    status: KalshiOrderStatus | str = "resting"
    yes_price_dollars: KalshiFixedPrice | None = None
    no_price_dollars: KalshiFixedPrice | None = None
    fill_count_fp: KalshiFixedCount | None = None
    remaining_count_fp: KalshiFixedCount | None = None
    initial_count_fp: KalshiFixedCount | None = None
    taker_fill_cost_dollars: KalshiFixedPrice | None = None
    maker_fill_cost_dollars: KalshiFixedPrice | None = None
    taker_fees_dollars: KalshiFixedPrice | None = None
    maker_fees_dollars: KalshiFixedPrice | None = None
    expiration_time: datetime | None = None
    created_time: datetime | None = None
    last_update_time: datetime | None = None
    self_trade_prevention_type: KalshiSelfTradePreventionType | str | None = None
    order_group_id: str | None = None
    cancel_order_on_pause: bool | None = None
    subaccount_number: int | None = None


@dataclass(frozen=True, slots=True)
class KalshiCreateOrderResponse:
    """Create-order response wrapper."""

    order: KalshiOrder | None = None


@dataclass(frozen=True, slots=True)
class KalshiGetOrderResponse:
    """Get-order response wrapper."""

    order: KalshiOrder | None = None


@dataclass(frozen=True, slots=True)
class KalshiGetOrdersResponse:
    """List-orders response wrapper."""

    orders: tuple[KalshiOrder, ...] = field(default_factory=tuple)
    cursor: str | None = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "orders", _freeze_tuple(self.orders))


@dataclass(frozen=True, slots=True)
class KalshiOrderbookLevel:
    """One aggregated price level in the Kalshi order book."""

    price_dollars: KalshiFixedPrice
    count_fp: KalshiFixedCount


@dataclass(frozen=True, slots=True)
class KalshiOrderbook:
    """Orderbook levels keyed by side.

    Both REST orderbook responses and websocket snapshots only carry bid-side
    levels for binary markets.
    """

    yes_dollars: tuple[KalshiOrderbookLevel, ...] = field(default_factory=tuple)
    no_dollars: tuple[KalshiOrderbookLevel, ...] = field(default_factory=tuple)

    def __post_init__(self) -> None:
        object.__setattr__(self, "yes_dollars", _freeze_tuple(self.yes_dollars))
        object.__setattr__(self, "no_dollars", _freeze_tuple(self.no_dollars))


@dataclass(frozen=True, slots=True)
class KalshiMarketOrderbook:
    """Ticker-scoped orderbook entry used by multi-market REST reads."""

    ticker: str
    orderbook_fp: KalshiOrderbook


@dataclass(frozen=True, slots=True)
class KalshiGetOrderbookResponse:
    """REST orderbook response wrapper."""

    orderbook_fp: KalshiOrderbook


@dataclass(frozen=True, slots=True)
class KalshiGetOrderbooksResponse:
    """Multi-market REST orderbook response wrapper."""

    orderbooks: tuple[KalshiMarketOrderbook, ...] = field(default_factory=tuple)

    def __post_init__(self) -> None:
        object.__setattr__(self, "orderbooks", _freeze_tuple(self.orderbooks))


@dataclass(frozen=True, slots=True)
class KalshiPriceRange:
    """Valid price interval and step for a market's active price structure."""

    start: KalshiFixedPrice
    end: KalshiFixedPrice
    step: KalshiFixedPrice


@dataclass(frozen=True, slots=True)
class KalshiMveSelectedLeg:
    """Selected multivariate leg returned on market payloads."""

    event_ticker: str = ""
    market_ticker: str = ""
    side: str = ""
    yes_settlement_value_dollars: KalshiFixedPrice | None = None


@dataclass(frozen=True, slots=True)
class KalshiMarket:
    """Market REST payload for list/get operations."""

    ticker: str = ""
    event_ticker: str = ""
    market_type: str = "binary"
    status: KalshiMarketStatus | str = "open"
    title: str = ""
    subtitle: str = ""
    yes_sub_title: str = ""
    no_sub_title: str = ""
    created_time: datetime | None = None
    updated_time: datetime | None = None
    open_time: datetime | None = None
    close_time: datetime | None = None
    latest_expiration_time: datetime | None = None
    expected_expiration_time: datetime | None = None
    expiration_time: datetime | None = None
    settlement_timer_seconds: int | None = None
    result: KalshiMarketResult | str | None = None
    can_close_early: bool | None = None
    fractional_trading_enabled: bool | None = None
    yes_bid_dollars: KalshiFixedPrice | None = None
    yes_bid_size_fp: KalshiFixedCount | None = None
    yes_ask_dollars: KalshiFixedPrice | None = None
    yes_ask_size_fp: KalshiFixedCount | None = None
    no_bid_dollars: KalshiFixedPrice | None = None
    no_ask_dollars: KalshiFixedPrice | None = None
    last_price_dollars: KalshiFixedPrice | None = None
    volume_fp: KalshiFixedCount | None = None
    volume_24h_fp: KalshiFixedCount | None = None
    open_interest_fp: KalshiFixedCount | None = None
    dollar_volume: int | None = None
    dollar_open_interest: int | None = None
    notional_value_dollars: KalshiFixedPrice | None = None
    liquidity_dollars: KalshiFixedPrice | None = None
    previous_yes_bid_dollars: KalshiFixedPrice | None = None
    previous_yes_ask_dollars: KalshiFixedPrice | None = None
    previous_price_dollars: KalshiFixedPrice | None = None
    expiration_value: str | None = None
    rules_primary: str | None = None
    rules_secondary: str | None = None
    response_price_units: str | None = None
    settlement_value_dollars: KalshiFixedPrice | None = None
    settlement_ts: datetime | None = None
    fee_waiver_expiration_time: datetime | None = None
    early_close_condition: str | None = None
    price_level_structure: str | None = None
    price_ranges: tuple[KalshiPriceRange, ...] = field(default_factory=tuple)
    tick_size: int | None = None
    strike_type: str | None = None
    floor_strike: int | None = None
    cap_strike: int | None = None
    functional_strike: str | None = None
    custom_strike: Mapping[str, object] = field(default_factory=dict)
    mve_collection_ticker: str | None = None
    mve_selected_legs: tuple[KalshiMveSelectedLeg, ...] = field(default_factory=tuple)
    primary_participant_key: str | None = None
    is_provisional: bool | None = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "price_ranges", _freeze_tuple(self.price_ranges))
        object.__setattr__(self, "custom_strike", _freeze_mapping(self.custom_strike))
        object.__setattr__(self, "mve_selected_legs", _freeze_tuple(self.mve_selected_legs))


@dataclass(frozen=True, slots=True)
class KalshiGetMarketResponse:
    """Single-market REST response wrapper."""

    market: KalshiMarket | None = None


@dataclass(frozen=True, slots=True)
class KalshiMarketsPage:
    """List-markets response wrapper."""

    markets: tuple[KalshiMarket, ...] = field(default_factory=tuple)
    cursor: str | None = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "markets", _freeze_tuple(self.markets))


@dataclass(frozen=True, slots=True)
class KalshiSubscribeCommand:
    """Base subscribe command for the main WebSocket connection."""

    id: int
    channels: tuple[KalshiWsChannel | str, ...]
    market_ticker: str | None = None
    market_tickers: tuple[str, ...] = field(default_factory=tuple)
    market_id: str | None = None
    market_ids: tuple[str, ...] = field(default_factory=tuple)

    def __post_init__(self) -> None:
        object.__setattr__(self, "channels", _freeze_tuple(self.channels))
        object.__setattr__(self, "market_tickers", _freeze_tuple(self.market_tickers))
        object.__setattr__(self, "market_ids", _freeze_tuple(self.market_ids))


@dataclass(frozen=True, slots=True)
class KalshiUnsubscribeCommand:
    """Unsubscribe command by subscription id."""

    id: int
    sids: tuple[int, ...]

    def __post_init__(self) -> None:
        object.__setattr__(self, "sids", _freeze_tuple(self.sids))


@dataclass(frozen=True, slots=True)
class KalshiListSubscriptionsCommand:
    """List-subscriptions command."""

    id: int


@dataclass(frozen=True, slots=True)
class KalshiUpdateSubscriptionCommand:
    """Update-subscription command for adding or removing tracked markets."""

    id: int
    action: KalshiSubscriptionUpdateAction | str
    market_tickers: tuple[str, ...]
    sid: int | None = None
    sids: tuple[int, ...] = field(default_factory=tuple)

    def __post_init__(self) -> None:
        object.__setattr__(self, "market_tickers", _freeze_tuple(self.market_tickers))
        object.__setattr__(self, "sids", _freeze_tuple(self.sids))


@dataclass(frozen=True, slots=True)
class KalshiOrderbookSnapshotMessage:
    """WebSocket orderbook snapshot payload."""

    sid: int
    seq: int
    market_ticker: str
    market_id: str
    yes_dollars_fp: tuple[KalshiOrderbookLevel, ...] = field(default_factory=tuple)
    no_dollars_fp: tuple[KalshiOrderbookLevel, ...] = field(default_factory=tuple)

    def __post_init__(self) -> None:
        object.__setattr__(self, "yes_dollars_fp", _freeze_tuple(self.yes_dollars_fp))
        object.__setattr__(self, "no_dollars_fp", _freeze_tuple(self.no_dollars_fp))


@dataclass(frozen=True, slots=True)
class KalshiOrderbookDeltaMessage:
    """WebSocket incremental orderbook update payload."""

    sid: int
    seq: int
    market_ticker: str
    market_id: str
    price_dollars: KalshiFixedPrice
    delta_fp: KalshiFixedCount
    side: KalshiMarketSide | str
    client_order_id: str | None = None
    subaccount: int | None = None
    ts: datetime | None = None


@dataclass(frozen=True, slots=True)
class KalshiTickerMessage:
    """WebSocket market ticker update with top-of-book and volume stats."""

    sid: int
    market_ticker: str
    market_id: str
    price_dollars: KalshiFixedPrice | None = None
    yes_bid_dollars: KalshiFixedPrice | None = None
    yes_ask_dollars: KalshiFixedPrice | None = None
    volume_fp: KalshiFixedCount | None = None
    open_interest_fp: KalshiFixedCount | None = None
    dollar_volume: int | None = None
    dollar_open_interest: int | None = None
    yes_bid_size_fp: KalshiFixedCount | None = None
    yes_ask_size_fp: KalshiFixedCount | None = None
    last_trade_size_fp: KalshiFixedCount | None = None
    ts: int | None = None
    time: datetime | None = None


@dataclass(frozen=True, slots=True)
class KalshiTradeMessage:
    """WebSocket public trade update payload."""

    sid: int
    trade_id: str
    market_ticker: str
    yes_price_dollars: KalshiFixedPrice | None = None
    no_price_dollars: KalshiFixedPrice | None = None
    count_fp: KalshiFixedCount | None = None
    taker_side: KalshiMarketSide | str | None = None
    ts: int | None = None


@dataclass(frozen=True, slots=True)
class KalshiUserOrderMessage:
    """WebSocket user-order update payload."""

    sid: int
    order_id: str
    user_id: str
    ticker: str
    status: KalshiOrderStatus | str
    side: KalshiMarketSide | str
    is_yes: bool
    yes_price_dollars: KalshiFixedPrice | None = None
    fill_count_fp: KalshiFixedCount | None = None
    remaining_count_fp: KalshiFixedCount | None = None
    initial_count_fp: KalshiFixedCount | None = None
    taker_fill_cost_dollars: KalshiFixedPrice | None = None
    maker_fill_cost_dollars: KalshiFixedPrice | None = None
    client_order_id: str | None = None
    order_group_id: str | None = None
    self_trade_prevention_type: KalshiSelfTradePreventionType | str | None = None
    created_time: datetime | None = None
    expiration_time: datetime | None = None
    subaccount_number: int | None = None


@dataclass(frozen=True, slots=True)
class KalshiUserFillMessage:
    """WebSocket private fill update payload."""

    sid: int
    trade_id: str
    order_id: str
    market_ticker: str
    is_taker: bool
    side: KalshiMarketSide | str
    yes_price_dollars: KalshiFixedPrice | None = None
    count_fp: KalshiFixedCount | None = None
    fee_cost: KalshiFixedPrice | None = None
    action: KalshiOrderAction | str = "buy"
    ts: int | None = None
    client_order_id: str | None = None
    post_position_fp: KalshiFixedCount | None = None
    purchased_side: KalshiMarketSide | str | None = None
    subaccount: int | None = None


@dataclass(frozen=True, slots=True)
class KalshiMarketPositionMessage:
    """WebSocket private market-position update payload."""

    sid: int
    user_id: str
    market_ticker: str
    position_fp: KalshiFixedCount | None = None
    position_cost_dollars: KalshiFixedPrice | None = None
    realized_pnl_dollars: KalshiFixedPrice | None = None
    fees_paid_dollars: KalshiFixedPrice | None = None
    position_fee_cost_dollars: KalshiFixedPrice | None = None
    volume_fp: KalshiFixedCount | None = None


@dataclass(frozen=True, slots=True)
class KalshiMarketLifecycleMetadata:
    """Optional market metadata carried on lifecycle create events."""

    name: str = ""
    title: str = ""
    yes_sub_title: str = ""
    no_sub_title: str = ""
    rules_primary: str = ""
    rules_secondary: str = ""
    can_close_early: bool | None = None
    event_ticker: str = ""
    expected_expiration_ts: int | None = None
    strike_type: str | None = None
    floor_strike: float | None = None
    cap_strike: float | None = None
    custom_strike: Mapping[str, object] = field(default_factory=dict)

    def __post_init__(self) -> None:
        object.__setattr__(self, "custom_strike", _freeze_mapping(self.custom_strike))


@dataclass(frozen=True, slots=True)
class KalshiMarketLifecycleMessage:
    """WebSocket market lifecycle update payload."""

    sid: int
    event_type: KalshiMarketLifecycleEventType | str
    market_ticker: str
    open_ts: int | None = None
    close_ts: int | None = None
    result: KalshiMarketResult | str | None = None
    determination_ts: int | None = None
    settlement_value: KalshiFixedPrice | None = None
    settled_ts: int | None = None
    is_deactivated: bool | None = None
    fractional_trading_enabled: bool | None = None
    price_level_structure: KalshiPriceLevelStructure | str | None = None
    additional_metadata: KalshiMarketLifecycleMetadata | None = None


@dataclass(frozen=True, slots=True)
class KalshiEventLifecycleMessage:
    """WebSocket event creation payload."""

    sid: int
    event_ticker: str
    title: str
    subtitle: str
    collateral_return_type: KalshiCollateralReturnType | str = ""
    series_ticker: str = ""
    strike_date: int | None = None
    strike_period: str | None = None


type KalshiWsMessage = (
    KalshiOrderbookSnapshotMessage
    | KalshiOrderbookDeltaMessage
    | KalshiTickerMessage
    | KalshiTradeMessage
    | KalshiUserOrderMessage
    | KalshiUserFillMessage
    | KalshiMarketPositionMessage
    | KalshiMarketLifecycleMessage
    | KalshiEventLifecycleMessage
)
