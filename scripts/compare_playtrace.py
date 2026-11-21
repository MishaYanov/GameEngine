"""Compare named gameplay marks in two headless replay reports."""

import argparse
from itertools import zip_longest
import json
import os
from pathlib import Path
import re
import stat
import sys
import unicodedata


MAX_REPORT_BYTES = 16 * 1024 * 1024
MAX_MARKERS = 128
MARKER_LABEL = re.compile(r"[A-Za-z0-9_-]{1,64}")
MARKER_FIELDS = (
    "scene_id", "simulation_tick", "completed", "score",
    "has_courtyard_key", "route_gate_open",
)


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("JSON contains a duplicate field")
        result[key] = value
    return result


def _invalid_constant(value):
    raise ValueError(f"Invalid JSON constant {value}")


def read_local_json(path, limit):
    path = Path(path)
    metadata = path.lstat()
    if not stat.S_ISREG(metadata.st_mode) or (
        getattr(metadata, "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT
    ):
        raise ValueError(f"{path.name} must be an ordinary file")
    if not 0 < metadata.st_size <= limit:
        raise ValueError(f"{path.name} is empty or exceeds its {limit}-byte limit")
    flags = os.O_RDONLY | getattr(os, "O_BINARY", 0)
    flags |= getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0)
    with os.fdopen(os.open(path, flags), "rb") as source:
        if not stat.S_ISREG(os.fstat(source.fileno()).st_mode):
            raise ValueError(f"{path.name} must be an ordinary file")
        bytes_read = source.read(limit + 1)
    if not 0 < len(bytes_read) <= limit:
        raise ValueError(f"{path.name} is empty or exceeds its {limit}-byte limit")
    try:
        value = json.loads(
            bytes_read.decode("utf-8"), object_pairs_hook=_unique_object,
            parse_constant=_invalid_constant,
        )
    except (UnicodeError, ValueError, RecursionError) as error:
        raise ValueError(f"{path.name} is invalid JSON: {error}") from error
    return value, bytes_read


def valid_scene(value):
    return (
        isinstance(value, str)
        and 1 <= len(value) <= 256 and len(value.encode("utf-8")) <= 256
        and not any(
            character.isspace() or unicodedata.category(character) == "Cc"
            for character in value
        )
    )


def valid_tick(value):
    return type(value) is int and 0 <= value <= (1 << 64) - 1


def valid_score(value):
    return value is None or (type(value) is int and -(1 << 31) <= value < 1 << 31)


def valid_flag(value):
    return value is None or type(value) is bool


def marker_rows(report):
    if not isinstance(report, dict):
        raise ValueError("Expected a headless replay report")
    markers = report.get("markers", [])
    if not isinstance(markers, list) or len(markers) > MAX_MARKERS:
        raise ValueError("Expected at most 128 replay markers")
    seen = set()
    for row in markers:
        if not isinstance(row, dict) or set(row) != {"label", *MARKER_FIELDS}:
            raise ValueError("Invalid marker fields")
        label = row["label"]
        if not isinstance(label, str) or not MARKER_LABEL.fullmatch(label) or label in seen:
            raise ValueError("Expected distinct replay marker labels")
        seen.add(label)
        if (
            not valid_scene(row["scene_id"])
            or not valid_tick(row["simulation_tick"])
            or not valid_score(row["score"])
            or not all(valid_flag(row[key]) for key in (
                "completed", "has_courtyard_key", "route_gate_open"
            ))
        ):
            raise ValueError(f"Invalid gameplay facts for marker {label!r}")
    return markers


def first_difference(expected, observed):
    for index, (left, right) in enumerate(zip_longest(expected, observed)):
        if left == right:
            continue
        if left is None or right is None:
            return {"index": index, "kind": "length", "expected": left, "observed": right}
        if left["label"] != right["label"]:
            return {
                "index": index, "kind": "label",
                "expected": left["label"], "observed": right["label"],
            }
        return {
            "index": index, "kind": "facts", "label": left["label"],
            "fields": {
                field: {"expected": left[field], "observed": right[field]}
                for field in MARKER_FIELDS if left[field] != right[field]
            },
        }
    return None


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("expected", type=Path)
    parser.add_argument("observed", type=Path)
    args = parser.parse_args(argv)
    try:
        expected = marker_rows(read_local_json(args.expected, MAX_REPORT_BYTES)[0])
        observed = marker_rows(read_local_json(args.observed, MAX_REPORT_BYTES)[0])
        if not expected and not observed:
            raise ValueError("At least one report must contain replay markers")
    except (OSError, ValueError) as error:
        parser.exit(2, f"Cannot compare playtraces. {error}\n")
    change = first_difference(expected, observed)
    json.dump({"match": change is None, "difference": change}, sys.stdout)
    print()
    return 1 if change else 0


if __name__ == "__main__":
    raise SystemExit(main())
