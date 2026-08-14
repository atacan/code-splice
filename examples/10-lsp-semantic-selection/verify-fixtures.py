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
    assert len(composed["operations"]) == 1

    with tempfile.TemporaryDirectory() as temporary:
        first_selection_path = Path(temporary) / "first-selection.json"
        first_selection_path.write_text(
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
        second_selection_path = Path(temporary) / "second-selection.json"
        second_selection_path.write_text(
            json.dumps(
                {
                    "matches": [
                        {
                            "request_source": {
                                "path": "src/source.rs",
                                "selector": {"kind": "bytes", "start": 7, "end": 11},
                                "precondition": {"kind": "sha256", "value": "sha256:abc"},
                            }
                        }
                    ]
                }
            ),
            encoding="utf-8",
        )
        request = subprocess.check_output(
            [
                sys.executable,
                str(ROOT / "compose-request.py"),
                str(first_selection_path),
                "src/one.rs",
                str(second_selection_path),
                "src/two.rs",
            ],
            text=True,
        )
    multi_composed = json.loads(request)
    assert len(multi_composed["operations"]) == 2
    assert [operation["source"] for operation in multi_composed["operations"]] == [
        composed["operations"][0]["source"],
        {
            "path": "src/source.rs",
            "selector": {"kind": "bytes", "start": 7, "end": 11},
            "precondition": {"kind": "sha256", "value": "sha256:abc"},
        },
    ]
    assert [operation["destination"]["path"] for operation in multi_composed["operations"]] == [
        "src/one.rs",
        "src/two.rs",
    ]

    for language, source, destination, declaration in (
        ("rust", "src/lib.rs", "src/extracted.rs", b"pub fn select_greeting"),
        ("python", "src/example.py", "src/extracted.py", b"def select_greeting"),
        ("typescript", "src/example.ts", "src/extracted.ts", b"export function selectGreeting"),
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

    swift_before = ROOT / "before" / "swift"
    swift_expected = ROOT / "expected" / "swift"
    swift_source = Path("Sources/SemanticDemo/SemanticDemo.swift")
    swift_original = (swift_before / swift_source).read_bytes()
    swift_declarations = (
        (b"public protocol DisplayNamed", Path("Sources/SemanticDemo/DisplayNamed.swift")),
        (b"public struct Account", Path("Sources/SemanticDemo/Account.swift")),
        (b"public extension Account", Path("Sources/SemanticDemo/Account+Greeting.swift")),
        (
            b"public extension DisplayNamed",
            Path("Sources/SemanticDemo/DisplayNamed+Formatting.swift"),
        ),
    )
    starts = [swift_original.index(marker) for marker, _ in swift_declarations]
    assert (swift_expected / swift_source).read_bytes() == swift_original[: starts[0]], (
        "swift expected source is not the exact prefix before the first moved declaration"
    )
    for index, (_, destination) in enumerate(swift_declarations):
        end = starts[index + 1] if index + 1 < len(starts) else len(swift_original)
        assert not (swift_before / destination).exists(), "swift destination must start absent"
        assert (swift_expected / destination).read_bytes() == swift_original[starts[index] : end], (
            f"swift expected {destination} is not the exact moved declaration"
        )

    print("semantic-selection fixtures are valid")


if __name__ == "__main__":
    main()
