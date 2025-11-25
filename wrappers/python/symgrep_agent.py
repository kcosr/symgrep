"""
Thin Python wrapper around the `symgrep` CLI.

The main entry point is `run_symgrep_search(config) -> dict`, where
`config` is a mapping whose keys correspond to CLI flags:

- `pattern` (str, required)
- `paths` (str or sequence of str, optional; defaults to ["."])
- `mode` ("text" | "symbol" | "auto", optional)
- `view` (sequence of "meta" | "decl" | "def" | "parent" | "comment" | "matches", optional)
- `language` (str, optional)
- `limit` (int, optional)
- `max_lines` (int, optional)
- `globs` (sequence of str, optional)
- `exclude_globs` (sequence of str, optional)
- `use_index` (bool, optional)
- `index_backend` ("file" | "sqlite", optional)
- `index_path` (str, optional)
- `literal` (bool, optional; when true, enables whole-identifier
  matching in text mode and exact symbol-name matching in symbol
  mode)
- `reindex_on_search` (bool, optional; when true, rebuilds or updates
  the index before running symbol-mode searches that use an index)
- `symgrep_bin` (str, optional; defaults to "symgrep")

The function spawns `symgrep` as a subprocess with `--format json`
and returns the decoded JSON payload as a `dict`. On non-zero exit
or invalid JSON, it raises `RuntimeError` with stderr attached.
"""

from __future__ import annotations

import json
import subprocess
from typing import Any, Dict, Iterable, Mapping, Sequence

__all__ = ["run_symgrep_search"]


def _as_paths(value: Any) -> Sequence[str]:
    if value is None:
        return ["."]
    if isinstance(value, str):
        return [value]
    try:
        return [str(v) for v in value]  # type: ignore[arg-type]
    except TypeError:
        return [str(value)]


def _as_strings(value: Any) -> Iterable[str]:
    if value is None:
        return []
    if isinstance(value, str):
        return [value]
    try:
        return [str(v) for v in value]  # type: ignore[arg-type]
    except TypeError:
        return [str(value)]


def run_symgrep_search(config: Mapping[str, Any]) -> Dict[str, Any]:
    """
    Run `symgrep search` with `--format json` and return the decoded result.

    Parameters
    ----------
    config:
        Mapping of options. See module docstring for supported keys.

    Returns
    -------
    dict
        Parsed JSON search result.

    Raises
    ------
    ValueError
        If the required `pattern` key is missing or empty.
    RuntimeError
        If the subprocess exits non-zero or emits invalid JSON.
    """

    pattern = str(config.get("pattern", "")).strip()
    if not pattern:
        raise ValueError("config['pattern'] is required and must be non-empty")

    paths = _as_paths(config.get("paths"))

    args = ["search", pattern]
    for p in paths:
        args.extend(["--path", p])

    mode = config.get("mode")
    if mode:
        args.extend(["--mode", str(mode)])

    view = config.get("view")
    if view:
        # Accept a single string, a comma-separated string, or a
        # sequence of view tokens and forward them to --view.
        from_values = _as_strings(view)
        for v in from_values:
            args.extend(["--view", str(v)])

    language = config.get("language")
    if language:
        args.extend(["--language", str(language)])

    if "limit" in config and config.get("limit") is not None:
        args.extend(["--limit", str(int(config["limit"]))])

    if "max_lines" in config and config.get("max_lines") is not None:
        args.extend(["--max-lines", str(int(config["max_lines"]))])

    for g in _as_strings(config.get("globs")):
        args.extend(["--glob", g])

    for g in _as_strings(config.get("exclude_globs")):
        args.extend(["--exclude", g])

    if bool(config.get("literal")):
        args.append("--literal")

    if bool(config.get("use_index")):
        args.append("--use-index")
        index_backend = config.get("index_backend")
        if index_backend:
            args.extend(["--index-backend", str(index_backend)])
        index_path = config.get("index_path")
        if index_path:
            args.extend(["--index-path", str(index_path)])

    if bool(config.get("reindex_on_search")):
        args.append("--reindex-on-search")

    args.extend(["--format", "json"])

    symgrep_bin = str(config.get("symgrep_bin", "symgrep"))

    proc = subprocess.run(
        [symgrep_bin, *args],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )

    if proc.returncode != 0:
        stderr = proc.stderr.strip()
        raise RuntimeError(
            f"symgrep exited with code {proc.returncode}"
            + (f": {stderr}" if stderr else "")
        )

    try:
        return json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"failed to parse symgrep JSON: {exc}") from exc
