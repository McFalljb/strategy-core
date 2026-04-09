"""Tests for the shared broker-facing contract."""

from __future__ import annotations

import pytest

from strategy_core import Broker, PendingOrder
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
        signal_type="demo",
    )

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
