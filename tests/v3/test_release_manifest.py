from __future__ import annotations

import hashlib
import importlib.util
import json
import re
from pathlib import Path
from typing import TYPE_CHECKING, Any, cast

if TYPE_CHECKING:
    from types import ModuleType

import pytest

ROOT = Path(__file__).parents[2]
RELEASE = ROOT / "release" / "v3"
SCRIPT = ROOT / "scripts" / "build_v3_release.py"


def _release_module() -> ModuleType:
    spec = importlib.util.spec_from_file_location("build_v3_release", SCRIPT)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _json(path: Path) -> dict[str, Any]:
    return cast("dict[str, Any]", json.loads(path.read_text()))


def test_release_manifest_pins_artifacts_corpus_semantics_and_exact_toolchains() -> None:
    manifest = _json(RELEASE / "manifest.json")
    toolchains = _json(RELEASE / "toolchains.json")

    assert manifest["schema_version"] == 1
    assert manifest["release"] == {
        "name": "strategy-core-v3",
        "version": "0.1.0",
        "tag": "strategy-core-v3-v0.1.0",
        "tag_type": "annotated",
    }
    assert manifest["canonical_profile"] == "strategy-core-canonical-v1"
    assert toolchains["python"]["implementation"] == "CPython"
    assert toolchains["python"]["version"] == "3.12.12"
    assert toolchains["python"]["configuration"]["path"] == ".python-version"
    assert toolchains["uv"]["version"] == "0.9.28"
    assert toolchains["rust"]["channel"] == "1.85.0"
    assert toolchains["rust"]["components"] == ["rustfmt"]
    assert toolchains["rust"]["configuration"]["path"] == "rust-toolchain.toml"
    assert toolchains["qualification_runner"] == {
        "architecture": "X64",
        "github_hosted_image": "ubuntu-24.04",
        "image_os": "ubuntu24",
        "os": "Linux",
    }
    assert "no cross-OS or cross-compressor" in toolchains["reproducibility"]["claim_scope"]
    assert manifest["rust_consumer_lock"]["path"] == "release/v3/rust-consumer.Cargo.lock"

    for record in [
        *manifest["artifacts"],
        *manifest["corpus"],
        *manifest["semantic_sources"],
        manifest["rust_consumer_lock"],
    ]:
        path = ROOT / record["path"]
        assert path.is_file(), record["path"]
        assert record["sha256"] == hashlib.sha256(path.read_bytes()).hexdigest()
        assert record["size"] == path.stat().st_size

    canonical_corpus = ROOT / "conformance" / "v3" / "vectors.json"
    released_corpus = RELEASE / "conformance" / "vectors.json"
    assert released_corpus.read_bytes() == canonical_corpus.read_bytes()


def test_sha256sums_covers_every_release_input_and_verifies() -> None:
    expected = {
        line.split("  ", 1)[1]: line.split("  ", 1)[0] for line in (RELEASE / "SHA256SUMS").read_text().splitlines()
    }
    files = {
        str(path.relative_to(RELEASE)) for path in RELEASE.rglob("*") if path.is_file() and path.name != "SHA256SUMS"
    }
    assert set(expected) == files
    for relative, digest in expected.items():
        assert hashlib.sha256((RELEASE / relative).read_bytes()).hexdigest() == digest


@pytest.mark.parametrize(
    "mutation",
    [
        {"url": "https://github.com/McFalljb/strategy-core/archive/refs/heads/main.zip"},
        {"url": "https://github.com/McFalljb/strategy-core.git", "branch": "main"},
        {"path": "../strategy-core"},
        {"url": "file:///tmp/strategy-core.whl"},
    ],
)
def test_qualification_consumer_rejects_moving_and_local_sources(mutation: dict[str, str]) -> None:
    module = _release_module()
    lock = _json(RELEASE / "consumer-lock.json")
    candidate = dict(lock["artifacts"][0])
    candidate.update(mutation)
    with pytest.raises(ValueError, match="immutable release artifact"):
        module.validate_consumer_source(candidate, lock["release_tag"])


def test_checked_consumer_lock_uses_only_digest_pinned_release_urls() -> None:
    module = _release_module()
    lock = _json(RELEASE / "consumer-lock.json")
    assert lock["release_tag"] == "strategy-core-v3-v0.1.0"
    assert lock["rust_consumer_lock"]["path"] == "release/v3/rust-consumer.Cargo.lock"
    for source in lock["artifacts"]:
        module.validate_consumer_source(source, lock["release_tag"])
        assert len(source["sha256"]) == 64
        assert source["url"].startswith(
            "https://github.com/McFalljb/strategy-core/releases/download/strategy-core-v3-v0.1.0/"
        )


def test_rust_clean_consumer_uses_checked_lock_and_fetches_before_offline_test() -> None:
    checked_lock = (RELEASE / "rust-consumer.Cargo.lock").read_text()
    builder = SCRIPT.read_text()

    assert 'name = "strategy-core-v3-release-consumer"' in checked_lock
    assert 'name = "strategy-core-v3"' in checked_lock
    assert 'name = "sha2"\nversion = "0.10.9"' in checked_lock
    assert 'name = "unicode-normalization"\nversion = "0.1.25"' in checked_lock
    assert 'name = "serde_json"\nversion = "1.0.150"' in checked_lock
    assert 'shutil.copyfile(release / RUST_CONSUMER_LOCK, consumer / "Cargo.lock")' in builder
    assert '["cargo", "generate-lockfile"' not in builder
    assert builder.index('["cargo", "fetch", "--locked"') < builder.index('["cargo", "test", "--locked", "--offline"')


def test_write_scoped_release_workflow_is_pinned_and_fails_closed() -> None:
    workflow = (ROOT / ".github" / "workflows" / "release-v3.yml").read_text()
    action_references = re.findall(r"uses:\s+[^@\s]+@([^\s]+)", workflow)

    assert action_references
    assert all(re.fullmatch(r"[0-9a-f]{40}", reference) for reference in action_references)
    assert "persist-credentials: false" in workflow
    assert "runs-on: ubuntu-24.04" in workflow
    assert "STRATEGY_CORE_V3_IMMUTABLE_RELEASES_ATTESTATION" in workflow
    assert "STRATEGY_CORE_V3_PROTECTED_TAG_ATTESTATION" in workflow
    assert 'test "$IMMUTABLE_RELEASES_ATTESTATION" = "immutable-releases-enabled"' in workflow
    assert 'test "$PROTECTED_TAG_ATTESTATION" = "strategy-core-v3-tags-update-delete-blocked-no-bypass"' in workflow
    assert 'test "$(git rev-parse "$RELEASE_TAG^{tag}")" = "$GITHUB_SHA"' in workflow
    assert 'test "$(git rev-parse "$RELEASE_TAG^{commit}")" = "$(git rev-parse HEAD)"' in workflow
    assert "release/v3/README.md" in workflow
    assert "release/v3/rust-consumer.Cargo.lock" in workflow
