"""Engine-owned HTTP request/response interfaces for strategy code."""

from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Literal, Protocol, runtime_checkable

if TYPE_CHECKING:
    from strategy_core.models import JSONValue

HttpMethod = Literal["GET", "POST", "PUT", "PATCH", "DELETE"]
HttpHeaders = Mapping[str, str]
HttpParams = Mapping[str, str | int | float | bool | None]


@dataclass(frozen=True, slots=True)
class HttpRequest:
    """Normalized outbound HTTP request shape."""

    method: HttpMethod
    url: str
    headers: dict[str, str] = field(default_factory=dict)
    params: dict[str, str | int | float | bool | None] = field(default_factory=dict)
    json_body: JSONValue | None = None
    text_body: str | None = None
    timeout_seconds: float | None = None


@dataclass(frozen=True, slots=True)
class HttpResponse:
    """Normalized HTTP response shape returned to strategies."""

    status_code: int
    headers: dict[str, str] = field(default_factory=dict)
    text: str | None = None
    json_body: JSONValue | None = None


@runtime_checkable
class HttpClient(Protocol):
    """Engine-managed HTTP surface so runtimes can observe and control requests."""

    async def request(self, request: HttpRequest) -> HttpResponse: ...

    async def get(
        self,
        url: str,
        *,
        headers: HttpHeaders | None = None,
        params: HttpParams | None = None,
        timeout_seconds: float | None = None,
    ) -> HttpResponse: ...

    async def post(
        self,
        url: str,
        *,
        headers: HttpHeaders | None = None,
        params: HttpParams | None = None,
        json_body: JSONValue | None = None,
        text_body: str | None = None,
        timeout_seconds: float | None = None,
    ) -> HttpResponse: ...
