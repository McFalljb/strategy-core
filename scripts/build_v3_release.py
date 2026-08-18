#!/usr/bin/env python3
"""Build and verify the immutable Strategy Core V3 release payload."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path
from typing import Any, cast
from urllib.parse import urlparse

ROOT = Path(__file__).resolve().parents[1]
RELEASE = ROOT / "release" / "v3"
VERSION = "0.1.0"
TAG = f"strategy-core-v3-v{VERSION}"
REPOSITORY = "https://github.com/McFalljb/strategy-core"
SOURCE_DATE_EPOCH = "1786924800"
PYTHON_ARTIFACT = f"strategy_core_v3-{VERSION}-py3-none-any.whl"
RUST_ARTIFACT = f"strategy-core-v3-{VERSION}.crate"
RUST_CONSUMER_LOCK = "rust-consumer.Cargo.lock"
RUST_CONSUMER_LOCK_SOURCE = ROOT / "scripts" / "v3_rust_consumer.Cargo.lock"
SEMANTIC_SOURCES = [
    "strategy_core_v3/__init__.py",
    "strategy_core_v3/canonical.py",
    "strategy_core_v3/context.py",
    "strategy_core_v3/profile.py",
    "strategy_core_v3/result.py",
    "native/strategy_core_v3/src/lib.rs",
]
SEMANTIC_SURFACES = [
    "strategy-context",
    "decision-result",
    "profile-calculator",
    "reason-code",
    "order-intent",
    "timer-request-meaning",
    "evidence",
    "diagnostic",
    "canonical-bytes",
]


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _record(path: Path, *, relative_to: Path = ROOT, **extra: Any) -> dict[str, Any]:
    return {
        "path": path.relative_to(relative_to).as_posix(),
        "sha256": _sha256(path),
        "size": path.stat().st_size,
        **extra,
    }


def _release_record(path: Path, output: Path, **extra: Any) -> dict[str, Any]:
    return {
        "path": (Path("release/v3") / path.relative_to(output)).as_posix(),
        "sha256": _sha256(path),
        "size": path.stat().st_size,
        **extra,
    }


def _write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def _run(command: list[str], *, env: dict[str, str] | None = None, cwd: Path = ROOT) -> None:
    subprocess.run(command, cwd=cwd, env=env, check=True)


def _build_rust_crate(destination: Path) -> None:
    """Emit a canonical source crate without commit- or Cargo-version-dependent metadata."""
    prefix = f"strategy-core-v3-{VERSION}"
    files = {
        f"{prefix}/Cargo.toml": (ROOT / "native" / "strategy_core_v3" / "Cargo.toml").read_bytes(),
        f"{prefix}/LICENSE": (ROOT / "LICENSE").read_bytes(),
        f"{prefix}/src/lib.rs": (ROOT / "native" / "strategy_core_v3" / "src" / "lib.rs").read_bytes(),
    }
    directories = {prefix, f"{prefix}/src"}
    destination.parent.mkdir(parents=True, exist_ok=True)
    with (
        destination.open("wb") as raw,
        gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=int(SOURCE_DATE_EPOCH)) as compressed,
        tarfile.open(fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT) as archive,
    ):
        for name in sorted(directories):
            member = tarfile.TarInfo(name)
            member.type = tarfile.DIRTYPE
            member.mode = 0o755
            member.uid = member.gid = 0
            member.mtime = int(SOURCE_DATE_EPOCH)
            archive.addfile(member)
        for name, payload in sorted(files.items()):
            member = tarfile.TarInfo(name)
            member.mode = 0o644
            member.uid = member.gid = 0
            member.mtime = int(SOURCE_DATE_EPOCH)
            member.size = len(payload)
            archive.addfile(member, fileobj=__import__("io").BytesIO(payload))


def _build_artifacts(output: Path, work: Path) -> tuple[Path, Path]:
    artifacts = output / "artifacts"
    artifacts.mkdir(parents=True)
    wheel_output = work / "wheel"
    wheel_output.mkdir(parents=True)
    python_project = work / "python-project"
    shutil.copytree(ROOT / "strategy_core_v3", python_project / "strategy_core_v3")
    shutil.copyfile(ROOT / "LICENSE", python_project / "LICENSE")
    shutil.copyfile(ROOT / "docs" / "v3-contract.md", python_project / "README.md")
    (python_project / "pyproject.toml").write_text(
        """[build-system]
requires = ["hatchling==1.32.0"]
build-backend = "hatchling.build"

[project]
name = "strategy-core-v3"
version = "0.1.0"
description = "Pure, bounded, runtime-neutral Strategy Core V3 semantics"
readme = "README.md"
license = "MIT"
requires-python = ">=3.12"
dependencies = []

[tool.hatch.build.targets.wheel]
packages = ["strategy_core_v3"]
"""
    )
    environment = dict(os.environ)
    environment["SOURCE_DATE_EPOCH"] = SOURCE_DATE_EPOCH
    environment["CARGO_NET_OFFLINE"] = "true"

    _run(["uv", "build", "--wheel", "--out-dir", str(wheel_output)], env=environment, cwd=python_project)
    wheel = wheel_output / PYTHON_ARTIFACT
    if not wheel.is_file():
        raise RuntimeError(f"expected Python artifact was not built: {wheel}")
    shutil.copyfile(wheel, artifacts / PYTHON_ARTIFACT)

    _build_rust_crate(artifacts / RUST_ARTIFACT)
    return artifacts / PYTHON_ARTIFACT, artifacts / RUST_ARTIFACT


def _toolchains() -> dict[str, Any]:
    return {
        "schema_version": 1,
        "python": {
            "implementation": "CPython",
            "version": "3.12.12",
            "configuration": _record(ROOT / ".python-version"),
        },
        "uv": {"version": "0.9.28", "lock": _record(ROOT / "uv.lock")},
        "python_build_backend": {"name": "hatchling", "version": "1.32.0"},
        "qualification_runner": {
            "os": "Linux",
            "architecture": "X64",
            "github_hosted_image": "ubuntu-24.04",
            "image_os": "ubuntu24",
        },
        "rust": {
            "channel": "1.85.0",
            "rustc": "rustc 1.85.0 (4d91de4e4 2025-02-17)",
            "cargo": "cargo 1.85.0 (d73d2caf9 2024-12-31)",
            "profile": "minimal",
            "components": ["rustfmt"],
            "configuration": _record(ROOT / "rust-toolchain.toml"),
            "workspace_lock": _record(ROOT / "native" / "Cargo.lock"),
            "clean_consumer_lock": _record(RUST_CONSUMER_LOCK_SOURCE),
        },
        "reproducibility": {
            "source_date_epoch": int(SOURCE_DATE_EPOCH),
            "python_command": "uv build --wheel",
            "rust_command": "cargo fetch --locked; cargo test --locked --offline in a clean CARGO_HOME",
            "rust_archive_normalization": (
                "CPython stdlib gzip.GzipFile level 9 over tarfile USTAR_FORMAT; sorted paths; "
                "uid/gid 0; fixed mtime; no generated VCS metadata"
            ),
            "claim_scope": (
                "byte identity is qualified on the recorded runner family and exact toolchains; "
                "no cross-OS or cross-compressor byte-identity claim is made"
            ),
        },
    }


def _build_once(output: Path, work: Path) -> None:
    output.mkdir(parents=True)
    shutil.copyfile(RELEASE / "README.md", output / "README.md")
    shutil.copyfile(RUST_CONSUMER_LOCK_SOURCE, output / RUST_CONSUMER_LOCK)
    python_artifact, rust_artifact = _build_artifacts(output, work)

    corpus_output = output / "conformance"
    corpus_output.mkdir()
    for name in ("manifest.json", "vectors.json"):
        shutil.copyfile(ROOT / "conformance" / "v3" / name, corpus_output / name)
    _write_json(output / "toolchains.json", _toolchains())

    manifest = {
        "schema_version": 1,
        "release": {
            "name": "strategy-core-v3",
            "version": VERSION,
            "tag": TAG,
            "tag_type": "annotated",
        },
        "canonical_profile": "strategy-core-canonical-v1",
        "artifacts": [
            _release_record(
                python_artifact,
                output,
                ecosystem="python",
                media_type="application/zip",
                semantic_surfaces=SEMANTIC_SURFACES,
            ),
            _release_record(
                rust_artifact,
                output,
                ecosystem="rust",
                media_type="application/gzip",
                semantic_surfaces=SEMANTIC_SURFACES,
            ),
        ],
        "corpus": [
            _release_record(corpus_output / "manifest.json", output, role="conformance-manifest"),
            _release_record(corpus_output / "vectors.json", output, role="conformance-vectors"),
        ],
        "semantic_sources": [_record(ROOT / relative) for relative in SEMANTIC_SOURCES],
        "toolchain_record": _release_record(output / "toolchains.json", output),
        "rust_consumer_lock": _release_record(output / RUST_CONSUMER_LOCK, output),
    }
    _write_json(output / "manifest.json", manifest)

    artifacts = cast("list[dict[str, Any]]", manifest["artifacts"])
    lock = {
        "schema_version": 1,
        "release_tag": TAG,
        "artifacts": [
            {
                "ecosystem": artifact["ecosystem"],
                "name": Path(artifact["path"]).name,
                "sha256": artifact["sha256"],
                "url": f"{REPOSITORY}/releases/download/{TAG}/{Path(artifact['path']).name}",
            }
            for artifact in artifacts
        ],
        "rust_consumer_lock": _release_record(output / RUST_CONSUMER_LOCK, output),
    }
    _write_json(output / "consumer-lock.json", lock)

    sums = []
    for path in sorted(item for item in output.rglob("*") if item.is_file() and item.name != "SHA256SUMS"):
        sums.append(f"{_sha256(path)}  {path.relative_to(output).as_posix()}")
    (output / "SHA256SUMS").write_text("\n".join(sums) + "\n")


def validate_consumer_source(source: dict[str, Any], release_tag: str) -> None:
    allowed = {"ecosystem", "name", "sha256", "url"}
    url = source.get("url")
    digest = source.get("sha256")
    name = source.get("name")
    prefix = f"{REPOSITORY}/releases/download/{release_tag}/"
    valid = (
        set(source) == allowed
        and isinstance(url, str)
        and url.startswith(prefix)
        and urlparse(url).scheme == "https"
        and isinstance(name, str)
        and url == prefix + name
        and isinstance(digest, str)
        and re.fullmatch(r"[0-9a-f]{64}", digest) is not None
    )
    if not valid:
        raise ValueError("qualification requires a digest-pinned immutable release artifact")


def _compare_directories(first: Path, second: Path) -> None:
    first_files = {path.relative_to(first) for path in first.rglob("*") if path.is_file()}
    second_files = {path.relative_to(second) for path in second.rglob("*") if path.is_file()}
    if first_files != second_files:
        raise RuntimeError(f"release file sets differ: {first_files ^ second_files}")
    changed = [
        relative
        for relative in sorted(first_files)
        if (first / relative).read_bytes() != (second / relative).read_bytes()
    ]
    if changed:
        raise RuntimeError(f"release is not reproducible; changed files: {changed}")


def _verify_release_hashes(release: Path) -> None:
    lock = json.loads((release / "consumer-lock.json").read_text())
    for source in lock["artifacts"]:
        validate_consumer_source(source, lock["release_tag"])
        artifact = release / "artifacts" / source["name"]
        if _sha256(artifact) != source["sha256"]:
            raise RuntimeError(f"consumer artifact digest mismatch: {source['name']}")


def _prove_python_consumer(release: Path, work: Path) -> None:
    site = work / "python-consumer"
    site.mkdir(parents=True)
    with zipfile.ZipFile(release / "artifacts" / PYTHON_ARTIFACT) as archive:
        archive.extractall(site)
    environment = dict(os.environ)
    environment["PYTHONPATH"] = str(site)
    _run(
        [
            sys.executable,
            str(ROOT / "scripts" / "v3_python_consumer.py"),
            str(release / "conformance" / "vectors.json"),
        ],
        env=environment,
    )


def _prove_rust_consumer(release: Path, work: Path) -> None:
    unpacked = work / "rust-artifact"
    unpacked.mkdir(parents=True)
    with tarfile.open(release / "artifacts" / RUST_ARTIFACT, "r:gz") as archive:
        archive.extractall(unpacked, filter="data")
    crate = unpacked / f"strategy-core-v3-{VERSION}"
    consumer = work / "rust-consumer"
    (consumer / "tests").mkdir(parents=True)
    (consumer / "conformance").mkdir()
    manifest = (
        '[package]\nname = "strategy-core-v3-release-consumer"\nversion = "0.0.0"\nedition = "2024"\n'
        f'\n[dependencies]\nstrategy-core-v3 = {{ path = {json.dumps(str(crate))}, version = "={VERSION}" }}\n'
        '\n[dev-dependencies]\nserde_json = "=1.0.150"\n'
    )
    (consumer / "Cargo.toml").write_text(manifest)
    test_source = (ROOT / "native" / "strategy_core_v3" / "tests" / "conformance.rs").read_text()
    test_source = test_source.replace('join("../../conformance/v3/vectors.json")', 'join("conformance/vectors.json")')
    repository_manifest_check = """#[test]
fn crate_dependencies_match_exact_pure_allowlists() {
    let manifest = include_str!("../Cargo.toml");
    assert_eq!(
        dependencies(manifest, "dependencies"),
        BTreeSet::from(["sha2".to_owned(), "unicode-normalization".to_owned()])
    );
    assert_eq!(
        dependencies(manifest, "dev-dependencies"),
        BTreeSet::from(["serde_json".to_owned()])
    );
}

"""
    if repository_manifest_check not in test_source:
        raise RuntimeError("Rust conformance consumer fixture changed unexpectedly")
    test_source = test_source.replace(repository_manifest_check, "")
    (consumer / "tests" / "conformance.rs").write_text(test_source)
    shutil.copyfile(release / "conformance" / "vectors.json", consumer / "conformance" / "vectors.json")
    shutil.copyfile(release / RUST_CONSUMER_LOCK, consumer / "Cargo.lock")
    environment = dict(os.environ)
    environment["CARGO_HOME"] = str(work / "cargo-home")
    _run(["cargo", "fetch", "--locked", "--manifest-path", str(consumer / "Cargo.toml")], env=environment)
    environment["CARGO_NET_OFFLINE"] = "true"
    _run(["cargo", "test", "--locked", "--offline", "--manifest-path", str(consumer / "Cargo.toml")], env=environment)
    print("rust clean consumer: checked lock fetched on an empty CARGO_HOME; shared vectors passed")


def _check_toolchains() -> None:
    expected = _toolchains()
    installed_components = subprocess.check_output(
        ["rustup", "component", "list", "--installed"], text=True
    ).splitlines()
    actual = {
        "python_implementation": platform.python_implementation(),
        "python": platform.python_version(),
        "uv": subprocess.check_output(["uv", "--version"], text=True).strip().split()[-1],
        "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
        "cargo": subprocess.check_output(["cargo", "--version"], text=True).strip(),
        "runner_os": os.environ.get("RUNNER_OS"),
        "runner_architecture": os.environ.get("RUNNER_ARCH"),
        "runner_image": os.environ.get("STRATEGY_CORE_RELEASE_RUNNER_IMAGE"),
        "image_os": os.environ.get("ImageOS"),  # noqa: SIM112 - GitHub defines this exact name.
    }
    wanted = {
        "python_implementation": expected["python"]["implementation"],
        "python": expected["python"]["version"],
        "uv": expected["uv"]["version"],
        "rustc": expected["rust"]["rustc"],
        "cargo": expected["rust"]["cargo"],
        "runner_os": expected["qualification_runner"]["os"],
        "runner_architecture": expected["qualification_runner"]["architecture"],
        "runner_image": expected["qualification_runner"]["github_hosted_image"],
        "image_os": expected["qualification_runner"]["image_os"],
    }
    missing_components = [
        component
        for component in expected["rust"]["components"]
        if not any(installed.startswith(f"{component}-") for installed in installed_components)
    ]
    if actual != wanted or missing_components:
        raise RuntimeError(
            f"release environment mismatch: expected {wanted} and Rust components "
            f"{expected['rust']['components']}, got {actual} and missing {missing_components}"
        )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="compare a clean rebuild with the checked payload")
    parser.add_argument("--check-toolchains", action="store_true", help="require the exact recorded toolchain versions")
    parser.add_argument("--skip-consumer-proofs", action="store_true")
    arguments = parser.parse_args()

    if arguments.check_toolchains:
        _check_toolchains()
    with tempfile.TemporaryDirectory(prefix="strategy-core-v3-release-") as temporary:
        temp = Path(temporary)
        first = temp / "first" / "release" / "v3"
        second = temp / "second" / "release" / "v3"
        _build_once(first, temp / "first-work")
        _build_once(second, temp / "second-work")
        _compare_directories(first, second)
        if arguments.check:
            _compare_directories(first, RELEASE)
            selected = RELEASE
        else:
            shutil.rmtree(RELEASE)
            shutil.copytree(first, RELEASE)
            selected = RELEASE
        _verify_release_hashes(selected)
        if not arguments.skip_consumer_proofs:
            _prove_python_consumer(selected, temp / "proof-python")
            _prove_rust_consumer(selected, temp / "proof-rust")
    print(f"Strategy Core V3 release {TAG} is reproducible and qualified for publish preflight")


if __name__ == "__main__":
    main()
