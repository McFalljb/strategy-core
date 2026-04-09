---
title: Initial Python project setup (trader/backtester-aligned)
type: feat
status: completed
date: 2026-04-08
---

# Initial Python project setup (trader/backtester-aligned)

## Overview

Bootstrap the empty `strategy-core` repository as a small reusable Python library with the same engineering conventions as the sibling `trader` and `backtester` repos: `uv` for installs, `ruff` for lint/format, `mypy` in strict mode, `pytest` for tests, a committed `uv.lock`, GitHub CI using frozen sync, and contributor-facing `AGENTS.md` / `README.md`.

Unlike `trader` and `backtester`, this repo is a shared contract package rather than a runtime application. The setup should therefore optimize for typed-library consumption, minimal runtime dependencies, and clean sibling-repo imports instead of engine commands, dashboards, or runtime infrastructure.

## Problem Statement / Motivation

`strategy-core` currently contains only `.git`. Without a consistent Python toolchain, CI contract, and basic contributor docs, the new shared package will drift from the repos that need to consume it and make later extraction/refactor work noisier than it needs to be.

Establishing the same baseline now gives three concrete wins:

- package work can begin immediately without repo-scaffolding churn
- `trader` and `backtester` can adopt the shared package through familiar tooling
- the shared contract starts life as a real typed library rather than an ad hoc folder copied between repos

## Proposed Solution

### 1. Project metadata and dependencies

- Add `pyproject.toml` modeled on the sibling repos:
  - `[build-system]`: use a standard library-friendly backend such as `hatchling`
  - `[project]`: `name = "strategy-core"` or the final decided distribution name, `version = "0.1.0"`, a short description, `requires-python = ">=3.12"`
  - `[dependency-groups].dev`: `pytest`, `pytest-asyncio`, `mypy`, `ruff`
  - Runtime `dependencies`: start minimal; the likely first runtime dependency is `pydantic>=2` for immutable typed event/value models. Do not copy runtime-specific packages like `httpx`, `aiosqlite`, `websockets`, `fastapi`, or `polars` unless the shared contract truly requires them.
  - `[tool.hatch.build.targets.wheel]` or equivalent packaging config should point at the import package directory
  - `[tool.ruff]`, `[tool.ruff.lint]`, `[tool.mypy]`, `[tool.pytest.ini_options]`: align with `trader` / `backtester` (`target-version = "py312"`, `line-length = 120`, strict mypy, `testpaths = ["tests"]`)

### 2. Package layout

- Create a flat import package at repo root:
  - `strategy_core/__init__.py`
  - `strategy_core/py.typed`
- Add `tests/test_smoke.py` that imports the package and proves the repo is a valid Python library from the start.
- Prefer the same overall layout style as the sibling repos rather than a `src/` layout unless packaging constraints later make that worthwhile.

### 3. Lockfile and local workflow

- Run `uv lock` / `uv sync` after `pyproject.toml` exists and commit `uv.lock`.
- Document in `AGENTS.md` and `README.md` that dependency changes must update the lockfile in the same PR.
- CI should use `uv sync --frozen --group dev`.

### 4. Repository hygiene

- Add a trimmed `.gitignore` based on `trader/.gitignore`:
  - keep Python, virtualenv, cache, IDE, env, and macOS patterns
  - omit SQLite/database, PEM, gzip, and runtime-artifact ignores unless the shared package actually generates them
- Add `.python-version` pinned to `3.12` for consistency if you want the new repo to match the local dev default used elsewhere.

### 5. GitHub automation

- Add `.github/workflows/ci.yml` mirroring the existing sibling repos:
  - `actions/checkout@v4`
  - `astral-sh/setup-uv@v8.0.0`
  - `uv sync --frozen --group dev`
  - `uv run ruff check .`
  - `uv run ruff format --check .`
  - `uv run mypy .`
  - `uv run pytest`
  - lint/typecheck on Python `3.12`
  - test matrix on Python `3.12` and `3.13`
- Add `.github/dependabot.yml` for GitHub Actions weekly, matching the current convention in `trader`.
- Ensure `on.push.branches` includes the actual default branch for `strategy-core`.

### 6. Contributor docs

- Add `AGENTS.md` with accurate commands and library-specific conventions:
  - installation via `uv sync`
  - local gate via `uv run ruff check . && uv run ruff format --check . && uv run mypy . && uv run pytest`
  - tests live in `tests/`
  - type annotations required
  - this is a shared typed library, not an engine/runtime repo
- Add a concise `README.md` covering:
  - repo purpose
  - prerequisites (`uv`, Python 3.12+)
  - quick start
  - full local gate
  - high-level package role as the shared strategy contract layer for sibling repos

### 7. Docs conventions

- Create `docs/plans/` and `docs/brainstorms/` inside `strategy-core` so future planning work can live with the repo itself.
- Follow the same dated filename and YAML-frontmatter convention already used in `backtester`.

## Technical Considerations

- **Library-first setup**: this repo should build and import cleanly without needing `trader` or `backtester` on the Python path.
- **Dependency discipline**: avoid pulling runtime-only dependencies into the shared package too early; the shared contract should stay narrow and stable.
- **Typed package expectations**: include `py.typed` so downstream repos and editors treat the package as typed from day one.
- **Naming split**: repo/distribution/import names do not have to be identical. The repo can be `strategy-core`, the distribution can later change if needed, and the import package should remain valid Python such as `strategy_core`.
- **Scope control**: do not add runtime directories like `engine/`, `dashboard/`, `strategies/`, `data/`, or exchange/minutetemp client code as part of bootstrap.

## System-Wide Impact

- **Interaction graph**: none yet at runtime, but this repo will become a shared dependency for `trader`, `backtester`, and potentially a private strategies repo.
- **Error propagation**: N/A at scaffold stage.
- **State lifecycle**: N/A unless the library later adds generated artifacts or local tooling outputs.
- **API surface parity**: starting with a clean typed-library scaffold makes it easier to keep the shared contract consumer-neutral.
- **Integration tests**: the first meaningful integration is that a clean clone can install, type-check, and import the package successfully.

## Acceptance Criteria

- [x] `pyproject.toml` exists with strict tooling config aligned to the sibling repos and an explicit build backend for a library package.
- [x] `strategy_core/` exists as an importable typed package with `__init__.py` and `py.typed`.
- [x] `tests/test_smoke.py` exists and validates import/basic library wiring.
- [x] `uv.lock` is committed and `uv sync --frozen --group dev` succeeds on a fresh clone.
- [x] `.github/workflows/ci.yml` exists with the same general job shape as `trader` / `backtester`.
- [x] `.github/dependabot.yml` exists for GitHub Actions weekly updates.
- [x] `.gitignore` is present and trimmed appropriately for a library repo.
- [x] `AGENTS.md` and `README.md` exist and describe the repo accurately as a shared typed contract library.
- [x] `docs/plans/` and `docs/brainstorms/` exist for ongoing work.

## Success Metrics

- First push to the default branch runs green CI.
- A new machine can clone `strategy-core`, run `uv sync`, and pass the full local gate without undocumented setup.
- The repo is immediately ready for the next plan that introduces the shared contract modules themselves.

## Dependencies and Risks

- **Risk**: Copying `trader` conventions too literally drags in runtime-specific guidance or dependencies.
  Mitigation: explicitly rewrite docs/config for a library repo rather than a trading engine.
- **Risk**: The package starts with unnecessary heavy dependencies.
  Mitigation: keep runtime dependencies minimal and add only what the first contract modules truly need.
- **Risk**: Repo/distribution/import naming gets conflated.
  Mitigation: document the distinction early and keep import package naming valid/stable (`strategy_core`).
- **Dependency**: `uv` is required locally and in CI via `astral-sh/setup-uv`.

## Sources and References

- `/Users/mcfalljb/workspace/projects/trader/pyproject.toml` — dependency groups and tool configuration
- `/Users/mcfalljb/workspace/projects/backtester/pyproject.toml` — sibling library-style repo baseline
- `/Users/mcfalljb/workspace/projects/trader/AGENTS.md` — contributor commands and conventions
- `/Users/mcfalljb/workspace/projects/backtester/AGENTS.md` — current plan/doc conventions for a sibling repo
- `/Users/mcfalljb/workspace/projects/trader/.github/workflows/ci.yml` — CI job shape
- `/Users/mcfalljb/workspace/projects/trader/.github/dependabot.yml` — dependabot convention
- `/Users/mcfalljb/workspace/projects/trader/.gitignore` — base ignore patterns to trim for a library repo
- `/Users/mcfalljb/workspace/projects/backtester/docs/plans/2026-04-06-001-feat-initial-python-project-setup-plan.md` — prior sibling bootstrap plan
