# Strategy Core

Shared typed strategy-contract library for the sibling `trader` and `backtester` repos.

## Project structure

```text
strategy-core/
  strategy_core/  # Shared import package
  tests/          # Test suite
  docs/           # Plans and brainstorms
```

## Python stack

- **Package manager**: uv
- **Build backend**: hatchling
- **Type checking**: mypy (strict mode)
- **Linting / formatting**: ruff
- **Testing**: pytest

## Commands

```bash
# Install dependencies (commits include uv.lock; CI uses --frozen)
uv sync

# Run tests
uv run pytest

# Type checking
uv run mypy .

# Lint
uv run ruff check .

# Format
uv run ruff format .

# Format check (CI)
uv run ruff format --check .

# Run all checks (lint + format + typecheck + test)
uv run ruff check . && uv run ruff format --check . && uv run mypy . && uv run pytest
```

## Conventions

- All Python code must pass `ruff check`, `ruff format --check`, and `mypy --strict` with no errors
- Use type annotations on all function signatures
- Keep runtime dependencies minimal; this repo is a shared library, not an engine
- Tests live under `tests/` and should mirror source layout as the package grows
- Test files are named `test_*.py`
- When you change dependencies in `pyproject.toml`, run `uv lock` (or `uv sync`) and commit `uv.lock` in the same change

## Docs

- Plans: `docs/plans/` (dated filenames, YAML frontmatter)
- Brainstorms: `docs/brainstorms/`
