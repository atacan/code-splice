#!/usr/bin/env python3
"""Deterministically validate semantic-selection example fixtures offline."""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parent


def main() -> None:
    subprocess.run(["bash", "-n", str(ROOT / "run.sh")], check=True)
    with tempfile.TemporaryDirectory() as temporary:
        selection_path = Path(temporary) / "selection.json"
        selection_path.write_text(
            json.dumps(
                {
                    "matches": [
                        {
                            "request_source": {
                                "path": "src/source.rs",
                                "selector": {"kind": "bytes", "start": 2, "end": 5},
                                "precondition": {"kind": "sha256", "value": "sha256:abc"},
                            }
                        }
                    ]
                }
            ),
            encoding="utf-8",
        )
        request = subprocess.check_output(
            [sys.executable, str(ROOT / "compose-request.py"), str(selection_path), "src/destination.rs"],
            text=True,
        )
    composed = json.loads(request)
    assert composed["operations"][0]["kind"] == "move"
    assert composed["operations"][0]["source"] == {
        "path": "src/source.rs",
        "selector": {"kind": "bytes", "start": 2, "end": 5},
        "precondition": {"kind": "sha256", "value": "sha256:abc"},
    }
    assert composed["operations"][0]["destination"]["precondition"] == {"kind": "must_not_exist"}

    for language, source, destination, declaration in (
        ("rust", "src/lib.rs", "src/extracted.rs", b"pub fn select_greeting"),
        ("python", "src/example.py", "src/extracted.py", b"def select_greeting"),
        ("typescript", "src/example.ts", "src/extracted.ts", b"export function selectGreeting"),
        (
            "swift",
            "Sources/SemanticDemo/SemanticDemo.swift",
            "Sources/SemanticDemo/Extracted.swift",
            b"public func selectGreeting",
        ),
    ):
        before = ROOT / "before" / language
        expected = ROOT / "expected" / language
        assert (before / source).is_file(), f"missing {language} source fixture"
        assert not (before / destination).exists(), f"{language} destination must start absent"
        assert (expected / destination).is_file(), f"missing {language} expected destination"
        original = (before / source).read_bytes()
        declaration_start = original.index(declaration)
        assert (expected / source).read_bytes() == original[:declaration_start], (
            f"{language} expected source is not the exact prefix before the declaration"
        )
        assert (expected / destination).read_bytes() == original[declaration_start:], (
            f"{language} expected destination is not the exact declaration suffix"
        )

    print("semantic-selection fixtures are valid")


if __name__ == "__main__":
    main()
