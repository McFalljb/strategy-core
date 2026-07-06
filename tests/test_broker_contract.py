"""Tests for the shared broker-facing contract."""

from __future__ import annotations

from dataclasses import asdict

import pytest

from strategy_core import (
    Broker,
    BrokerOrderUpdate,
    BrokerUpdateStatus,
    OrderIntent,
    PendingOrder,
)
from tests.fakes import FakeBroker


@pytest.mark.asyncio
async def test_broker_protocol_supports_order_placement_and_views() -> None:
    broker = FakeBroker()
    assert isinstance(broker, Broker)

    result = await broker.place_order(
        ticker="KXHIGHMIA-26APR08-B70.5",
        action="buy",
        contract_side="yes",
        order_type="market",
        quantity=3,
        execution_style="resting_limit",
        time_policy="good_till_canceled",
        expires_after_ms=30_000,
        signal_type="demo",
    )

    assert broker.placed_order_calls[0]["expires_after_ms"] == 30_000
    assert result.status == "filled"
    position = broker.get_position("KXHIGHMIA-26APR08-B70.5", "yes")
    assert position is not None
    assert position.quantity == 3
    assert broker.get_sleeve_buying_power() == 100.0
    assert list(broker.get_positions()) == ["KXHIGHMIA-26APR08-B70.5:yes"]


@pytest.mark.asyncio
async def test_broker_cancel_methods_operate_on_pending_orders() -> None:
    broker = FakeBroker()
    broker.pending_orders.append(
        PendingOrder(
            order_id="1",
            sleeve_id="demo:KMIA",
            ticker="KXHIGHMIA-26APR08-B70.5",
            action="buy",
            contract_side="yes",
            limit_price=0.4,
            requested_quantity=1,
        ),
    )
    assert await broker.cancel_order("1") is True
    assert await broker.cancel_all_orders() == 0


def test_order_intent_and_update_model_direct_sweep_semantics() -> None:
    intent = OrderIntent(
        ticker="KXHIGHMIA-26APR08-B70.5",
        action="buy",
        contract_side="yes",
        order_type="market",
        quantity=5,
        max_price=0.61,
        max_cost=305.0,
        execution_style="sweep",
        time_policy="immediate_or_cancel",
        expires_after_ms=30_000,
        signal_type="demo",
        client_order_id="client-1",
    )

    assert asdict(intent)["execution_style"] == "sweep"
    assert asdict(intent)["expires_after_ms"] == 30_000
    assert intent.max_price == 0.61
    assert intent.signal_metadata is None

    update = BrokerOrderUpdate(
        order_id="order-1",
        sleeve_id="demo:KMIA",
        ticker=intent.ticker,
        status="partially_filled",
        action=intent.action,
        contract_side=intent.contract_side,
        requested_quantity=5,
        filled_quantity=3,
        remaining_quantity=2,
        average_fill_price=0.59,
        client_order_id=intent.client_order_id,
        provider_sequence="sid=13:seq=42",
        expires_at="2026-06-17T12:00:30Z",
    )

    assert update.status == "partially_filled"
    assert update.remaining_quantity == 2
    assert update.provider_sequence == "sid=13:seq=42"
    assert asdict(update)["expires_at"] == "2026-06-17T12:00:30Z"


def test_order_intent_positional_constructor_keeps_existing_field_order() -> None:
    intent = OrderIntent(
        "KXHIGHMIA-26APR08-B70.5",
        "buy",
        "yes",
        "market",
        5,
        None,
        0.61,
        305.0,
        "sweep",
        "immediate_or_cancel",
        True,
        False,
        "demo",
        '{"source":"test"}',
        "client-1",
    )

    assert intent.reduce_only is True
    assert intent.signal_type == "demo"
    assert intent.client_order_id == "client-1"
    assert intent.expires_after_ms is None


def test_broker_order_update_positional_constructor_keeps_existing_field_order() -> None:
    update = BrokerOrderUpdate(
        "order-1",
        "demo:KMIA",
        "KXHIGHMIA-26APR08-B70.5",
        "partially_filled",
        "buy",
        "yes",
        5,
        3,
        2,
        0.0,
        0.59,
        0.0,
        "partial",
        "client-1",
        "provider-order-1",
        "sid=13:seq=42",
        "2026-06-17T12:00:00Z",
    )

    assert update.updated_at == "2026-06-17T12:00:00Z"
    assert update.expires_at is None


def test_pending_order_preserves_expiry_deadline() -> None:
    pending = PendingOrder(
        order_id="order-1",
        sleeve_id="demo:KMIA",
        ticker="KXHIGHMIA-26APR08-B70.5",
        action="buy",
        contract_side="yes",
        limit_price=0.61,
        requested_quantity=3,
        expires_at="2026-06-17T12:00:30Z",
    )

    assert asdict(pending)["expires_at"] == "2026-06-17T12:00:30Z"


def test_broker_update_status_terminal_market_transitions() -> None:
    statuses: tuple[BrokerUpdateStatus, BrokerUpdateStatus] = ("expired", "closed")

    assert statuses == ("expired", "closed")
