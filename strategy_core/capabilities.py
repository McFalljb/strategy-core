"""Runtime capability flags that strategy code may branch on when needed."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

EventDelivery = Literal["wake", "decision"]


@dataclass(frozen=True, slots=True)
class RuntimeCapabilities:
    """Small first-cut capability set shared by paper and replay runtimes."""

    supports_http: bool = False
    supports_data_queries: bool = True
    supports_one_shot_timers: bool = False
    supports_recurring_timers: bool = False
    supports_native_kernels: bool = False
    queue_is_durable: bool = False
    replay_controls_event_progression: bool = False
    event_delivery: EventDelivery = "wake"
