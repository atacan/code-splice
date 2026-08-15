#!/usr/bin/env python3
"""Deterministically validate semantic-selection example fixtures offline."""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parent


def assert_moved_declarations(
    language: str, source: Path, declarations: tuple[tuple[bytes, Path], ...]
) -> None:
    before = ROOT / "before" / language
    expected = ROOT / "expected" / language
    original = (before / source).read_bytes()
    starts = [original.index(marker) for marker, _ in declarations]
    assert (expected / source).read_bytes() == original[: starts[0]], (
        f"{language} expected source is not the exact prefix before the first moved declaration"
    )
    for index, (_, destination) in enumerate(declarations):
        end = starts[index + 1] if index + 1 < len(starts) else len(original)
        assert not (before / destination).exists(), f"{language} destination must start absent"
        assert (expected / destination).read_bytes() == original[starts[index] : end], (
            f"{language} expected {destination} is not the exact moved declaration"
        )


def assert_position_queries(
    language: str, source: Path, positions: tuple[tuple[int, bytes], ...]
) -> None:
    lines = (ROOT / "before" / language / source).read_bytes().splitlines()
    for line_number, marker in positions:
        assert lines[line_number - 1].startswith(marker), (
            f"{language} position query line {line_number} no longer identifies {marker!r}"
        )


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

    assert_moved_declarations(
        "rust",
        Path("src/lib.rs"),
        (
            (b"pub trait Greets", Path("src/greets.rs")),
            (b"pub struct Person", Path("src/person.rs")),
            (b"impl Person", Path("src/person_inherent.rs")),
            (b"impl Greets for Person", Path("src/person_greets.rs")),
        ),
    )
    assert_position_queries(
        "rust",
        Path("src/lib.rs"),
        ((10, b"impl Person"), (15, b"impl Greets for Person")),
    )
    assert_moved_declarations(
        "python",
        Path("src/example.py"),
        (
            (b"class Named", Path("src/named.py")),
            (b"class Person", Path("src/person.py")),
            (b"class GreetingAdapter", Path("src/greeting_adapter.py")),
            (b"class UppercaseGreetingAdapter", Path("src/uppercase_greeting_adapter.py")),
        ),
    )
    assert_position_queries(
        "typescript",
        Path("src/example.ts"),
        ((10, b"export namespace Person"),),
    )
    assert_moved_declarations(
        "typescript",
        Path("src/example.ts"),
        (
            (b"export interface Named", Path("src/named.ts")),
            (b"export class Person", Path("src/person.ts")),
            (b"export namespace Person", Path("src/person-namespace.ts")),
            (b"export function formatGreeting", Path("src/format-greeting.ts")),
        ),
    )
    assert_position_queries(
        "swift",
        Path("Sources/SemanticDemo/SemanticDemo.swift"),
        (
            (4, b"public protocol DisplayNamed"),
            (7, b"public struct Account"),
            (13, b"public extension Account"),
            (18, b"public extension DisplayNamed"),
        ),
    )
    assert_moved_declarations(
        "swift",
        Path("Sources/SemanticDemo/SemanticDemo.swift"),
        (
            (b"public protocol DisplayNamed", Path("Sources/SemanticDemo/DisplayNamed.swift")),
            (b"public struct Account", Path("Sources/SemanticDemo/Account.swift")),
            (b"public extension Account", Path("Sources/SemanticDemo/Account+Greeting.swift")),
            (
                b"public extension DisplayNamed",
                Path("Sources/SemanticDemo/DisplayNamed+Formatting.swift"),
            ),
        ),
    )

    print("semantic-selection fixtures are valid")


if __name__ == "__main__":
    main()
