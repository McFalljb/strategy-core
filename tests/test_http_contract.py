"""Tests for the engine-owned HTTP contract."""

from __future__ import annotations

import pytest

from strategy_core import HttpClient, HttpRequest, HttpResponse
from tests.fakes import FakeHttpClient


@pytest.mark.asyncio
async def test_http_client_supports_request_get_and_post() -> None:
    client = FakeHttpClient()
    assert isinstance(client, HttpClient)

    get_response = await client.get("https://example.com/weather", params={"station": "KMIA"})
    post_response = await client.post("https://example.com/model", json_body={"station": "KMIA"})
    request_response = await client.request(HttpRequest(method="GET", url="https://example.com/ping"))

    assert get_response.status_code == 200
    assert post_response.json_body == {"method": "POST", "url": "https://example.com/model"}
    assert request_response.json_body == {"method": "GET", "url": "https://example.com/ping"}
    assert [request.url for request in client.requests] == [
        "https://example.com/weather",
        "https://example.com/model",
        "https://example.com/ping",
    ]


def test_http_models_accept_non_object_json_values() -> None:
    request = HttpRequest(method="POST", url="https://example.com/list", json_body=[{"station": "KMIA"}])
    response = HttpResponse(status_code=200, json_body=[1, 2, 3])

    assert request.json_body == [{"station": "KMIA"}]
    assert response.json_body == [1, 2, 3]
