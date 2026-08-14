#!/usr/bin/env python3
"""Build a protocol-v1 move request from one selection-v1 result.

The `request_source` object is assigned directly from the selection response.  Do
not add, remove, or rewrite any of its fields: its source digest binds a later
preview and commit to the exact snapshot observed by the language server.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("usage: compose-request.py SELECTION_JSON DESTINATION_PATH")

    selection = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    matches = selection.get("matches")
    if not isinstance(matches, list) or len(matches) != 1:
        raise SystemExit("selection must contain exactly one match")
    request_source = matches[0].get("request_source")
    if not isinstance(request_source, dict):
        raise SystemExit("selection match has no request_source object")

    request = {
        "protocol_version": 1,
        "operations": [
            {
                "kind": "move",
                "source": request_source,
                "destination": {
                    "path": sys.argv[2],
                    "anchor": {"kind": "file_start"},
                    "precondition": {"kind": "must_not_exist"},
                },
            }
        ],
    }
    json.dump(request, sys.stdout, indent=2)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
