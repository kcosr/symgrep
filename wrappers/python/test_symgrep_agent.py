import subprocess
from pathlib import Path
from typing import Any, Dict

from symgrep_agent import run_symgrep_search


REPO_ROOT = Path(__file__).resolve().parents[2]


def _build_symgrep() -> Path:
    subprocess.run(["cargo", "build"], check=True, cwd=REPO_ROOT)
    # Debug binary path; tests are deterministic as long as we build once.
    return REPO_ROOT / "target" / "debug" / "symgrep"


def test_python_wrapper_symbol_search_decl_context() -> None:
    symgrep_bin = _build_symgrep()
    ts_repo = REPO_ROOT / "tests" / "fixtures" / "ts_js_repo"

    result: Dict[str, Any] = run_symgrep_search(
        {
            "pattern": "add",
            "paths": [str(ts_repo)],
            "mode": "symbol",
            "context": "decl",
            "language": "typescript",
            "symgrep_bin": str(symgrep_bin),
        }
    )

    assert result["query"] == "add"
    assert result["version"]

    symbols = result.get("symbols") or []
    assert symbols, "expected at least one symbol result"

    names = {s["name"] for s in symbols}
    assert "add" in names

    contexts = result.get("contexts") or []
    assert contexts, "expected at least one context for add"
    first_snippet = contexts[0].get("snippet") or ""
    assert "export function add" in first_snippet


if __name__ == "__main__":
    # Simple ad-hoc runner so this test can be executed via:
    #   python wrappers/python/test_symgrep_agent.py
    test_python_wrapper_symbol_search_decl_context()
