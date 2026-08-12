#!/usr/bin/env python3
"""Materialize an example tree, decoding files whose names end in .hex."""

from __future__ import annotations

import shutil
import sys
from pathlib import Path


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("usage: materialize.py SOURCE DESTINATION")

    source = Path(sys.argv[1])
    destination = Path(sys.argv[2])
    if not source.is_dir():
        raise SystemExit(f"source tree does not exist: {source}")
    destination.mkdir(parents=True, exist_ok=False)

    for item in sorted(source.rglob("*")):
        relative = item.relative_to(source)
        if relative.name == ".gitkeep":
            continue
        if item.is_dir():
            (destination / relative).mkdir(parents=True, exist_ok=True)
            continue
        if not item.is_file():
            raise SystemExit(f"unsupported fixture type: {item}")

        if item.suffix == ".hex":
            output = destination / relative.with_suffix("")
            output.parent.mkdir(parents=True, exist_ok=True)
            try:
                output.write_bytes(bytes.fromhex(item.read_text(encoding="ascii")))
            except ValueError as error:
                raise SystemExit(f"invalid hexadecimal fixture {item}: {error}") from error
        else:
            output = destination / relative
            output.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(item, output)


if __name__ == "__main__":
    main()

