#!/usr/bin/env python3
"""Build a protocol-v1 move request from selection-v1 results.

The `request_source` object is assigned directly from the selection response.  Do
not add, remove, or rewrite any of its fields: its source digest binds a later
preview and commit to the exact snapshot observed by the language server.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path


def main() -> None:
    arguments = sys.argv[1:]
    if len(arguments) < 2 or len(arguments) % 2:
        raise SystemExit(
            "usage: compose-request.py SELECTION_JSON DESTINATION_PATH "
            "[SELECTION_JSON DESTINATION_PATH ...]"
        )

    operations = []
    for selection_name, destination_path in zip(arguments[::2], arguments[1::2]):
        selection = json.loads(Path(selection_name).read_text(encoding="utf-8"))
        matches = selection.get("matches")
        if not isinstance(matches, list) or len(matches) != 1:
            raise SystemExit(f"{selection_name}: selection must contain exactly one match")
        request_source = matches[0].get("request_source")
        if not isinstance(request_source, dict):
            raise SystemExit(f"{selection_name}: selection match has no request_source object")
        operations.append(
            {
                "kind": "move",
                "source": request_source,
                "destination": {
                    "path": destination_path,
                    "anchor": {"kind": "file_start"},
                    "precondition": {"kind": "must_not_exist"},
                },
            }
        )

    request = {
        "protocol_version": 1,
        "operations": operations,
    }
    json.dump(request, sys.stdout, indent=2)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
