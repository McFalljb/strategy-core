"""Shared broker protocols and value objects for strategy-facing order flow."""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal, Protocol, runtime_checkable

if TYPE_CHECKING:
    from strategy_core.models import OrderId

Action = Literal["buy", "sell"]
ContractSide = Literal["yes", "no"]
OrderType = Literal["market", "limit"]
OrderStatus = Literal["filled", "partial", "pending", "rejected", "cancelled"]


@dataclass(frozen=True, slots=True)
class Position:
    """Open position in a single ticker for one strategy sleeve."""

    ticker: str
    side: ContractSide
    quantity: int
    avg_price: float


@dataclass(frozen=True, slots=True)
class PendingOrder:
    """Limit order awaiting fill."""

    order_id: OrderId
    sleeve_id: str
    ticker: str
    action: Action
    contract_side: ContractSide
    limit_price: float
    requested_quantity: int
    filled_quantity: int = 0
    reserved_global: float = 0.0
    reserved_sleeve: float = 0.0
    fee_type: str = ""
    fee_multiplier: float | None = None
    fee_accumulator: float = 0.0
    signal_type: str | None = None
    signal_metadata: str | None = None
    created_at: str = ""
    client_order_id: str | None = None


@dataclass(frozen=True, slots=True)
class OrderResult:
    """Outcome returned after placing an order."""

    order_id: OrderId
    sleeve_id: str
    status: OrderStatus
    filled_quantity: int = 0
    fill_price: float = 0.0
    fee_cost: float = 0.0
    reason: str = ""


@runtime_checkable
class Broker(Protocol):
    """Strategy-facing broker surface implemented by runtimes."""

    async def place_order(
        self,
        *,
        ticker: str,
        action: Action,
        contract_side: ContractSide,
        order_type: OrderType,
        quantity: int,
        limit_price: float | None = None,
        signal_type: str | None = None,
        signal_metadata: str | None = None,
        client_order_id: str | None = None,
    ) -> OrderResult: ...

    async def cancel_order(self, order_id: OrderId) -> bool: ...

    async def cancel_all_orders(self) -> int: ...

    def get_position(self, ticker: str, side: ContractSide = "yes") -> Position | None: ...

    def get_positions(self) -> dict[str, Position]: ...

    def get_pending_orders(self) -> list[PendingOrder]: ...

    def get_sleeve_buying_power(self) -> float: ...
