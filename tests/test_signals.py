"""Tests for shared strategy signal identifiers."""

from strategy_core.signals import SIGNAL_DSM_REACTION, SIGNAL_METAR_6HR_LOW, SIGNAL_METAR_6HR_NEW_LOW


def test_signal_identifiers_are_stable() -> None:
    assert SIGNAL_DSM_REACTION == "dsm_reaction"
    assert SIGNAL_METAR_6HR_LOW == "metar_6hr_low"
    assert SIGNAL_METAR_6HR_NEW_LOW == "metar_6hr_new_low"
