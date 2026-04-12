"""Tests for Kalshi-aligned shared payload models."""

from __future__ import annotations

from dataclasses import FrozenInstanceError
from typing import Any, cast

import pytest

from strategy_core.kalshi import (
    KalshiGetOrderbookResponse,
    KalshiGetOrderbooksResponse,
    KalshiMarket,
    KalshiMarketLifecycleMetadata,
    KalshiMarketOrderbook,
    KalshiOrderbook,
    KalshiOrderbookLevel,
    KalshiOrderbookSnapshotMessage,
)


def test_kalshi_orderbook_response_freezes_nested_levels() -> None:
    response = KalshiGetOrderbookResponse(
        orderbook_fp=KalshiOrderbook(
            yes_dollars=cast(
                "Any",
                [KalshiOrderbookLevel(price_dollars="0.1500", count_fp="100.00")],
            ),
            no_dollars=cast(
                "Any",
                [KalshiOrderbookLevel(price_dollars="0.8500", count_fp="25.00")],
            ),
        ),
    )

    assert isinstance(response.orderbook_fp.yes_dollars, tuple)
    assert isinstance(response.orderbook_fp.no_dollars, tuple)

    with pytest.raises(AttributeError):
        cast("Any", response.orderbook_fp.yes_dollars).append(
            KalshiOrderbookLevel(price_dollars="0.2000", count_fp="50.00"),
        )


def test_kalshi_multi_market_orderbooks_response_freezes_collections() -> None:
    response = KalshiGetOrderbooksResponse(
        orderbooks=cast(
            "Any",
            [
                KalshiMarketOrderbook(
                    ticker="FED-24DEC-T3.00",
                    orderbook_fp=KalshiOrderbook(),
                ),
            ],
        ),
    )

    assert isinstance(response.orderbooks, tuple)

    with pytest.raises(AttributeError):
        cast("Any", response.orderbooks).append(
            KalshiMarketOrderbook(
                ticker="OTHER-24DEC-T4.00",
                orderbook_fp=KalshiOrderbook(),
            ),
        )


def test_kalshi_custom_strike_mappings_are_frozen() -> None:
    market = KalshiMarket(custom_strike=cast("Any", {"threshold": 53.5}))
    metadata = KalshiMarketLifecycleMetadata(custom_strike=cast("Any", {"threshold": 54.0}))

    with pytest.raises(TypeError):
        cast("Any", market.custom_strike)["other"] = 55.0

    with pytest.raises(TypeError):
        cast("Any", metadata.custom_strike)["other"] = 56.0


def test_kalshi_snapshot_message_is_frozen() -> None:
    message = KalshiOrderbookSnapshotMessage(
        sid=2,
        seq=3,
        market_ticker="FED-24DEC-T3.00",
        market_id="market-123",
    )

    with pytest.raises(FrozenInstanceError):
        cast("Any", message).sid = 4
