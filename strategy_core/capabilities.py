"""Runtime capability flags that strategy code may branch on when needed."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class RuntimeCapabilities:
    """Small first-cut capability set shared by paper and replay runtimes."""

    supports_http: bool = False
    supports_one_shot_timers: bool = False
    supports_recurring_timers: bool = False
    queue_is_durable: bool = False
    replay_controls_event_progression: bool = False
