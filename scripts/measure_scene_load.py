"""Calibrate or check one fixture's scene-load timings on a named host profile."""

import argparse
import json
import os
from pathlib import Path
import re
import stat
import statistics
import sys
from uuid import UUID

from compare_playtrace import _invalid_constant, _unique_object, read_local_json


MAX_LOG_BYTES = 64 * 1024 * 1024
MAX_LINE_BYTES = 64 * 1024
MAX_METADATA_BYTES = 16 * 1024
MAX_LOGS = 256
MAX_EVENTS = 1000
MIN_SAMPLES = 20
MAX_SAMPLES = 200
MAX_MICROSECONDS = (1 << 64) - 1
MAX_HEADROOM_PERCENT = 1000
MAX_JITTER_US = 1_000_000
SHA256 = re.compile(r"[a-f0-9]{64}")
HOSTS = ("player", "editor")
SCOPES = {"cold": "startup", "warm": "play", "authored": "authored"}
PHASES = ("cpu_us", "queue_us", "gpu_us", "presentation_us")
METRICS = ("first_present_us", *PHASES)
TIMING_FIELDS = {
    "schema_version", "request_id", "host", "scope", "outcome", "elapsed_us", *METRICS,
}
PROFILE_TEXT = ("os", "cpu", "gpu", "driver", "build")
PROFILE_SIZE = ("width", "height")


def integer(value, maximum, minimum=0):
    return type(value) is int and minimum <= value <= maximum


def fixture_id(value):
    if not isinstance(value, str) or not SHA256.fullmatch(value):
        raise ValueError("Fixture identity must be a lowercase SHA-256 digest")
    return value


def validate_profile(profile):
    if not isinstance(profile, dict) or set(profile) != {*PROFILE_TEXT, *PROFILE_SIZE}:
        raise ValueError("Expected OS, CPU, GPU, driver, build and drawable size in profile")
    for label in PROFILE_TEXT:
        value = profile[label]
        if (not isinstance(value, str) or not value.strip()
                or not value.isprintable() or len(value.encode("utf-8")) > 256):
            raise ValueError(f"Invalid scene-load profile field {label!r}")
    for label in PROFILE_SIZE:
        if not integer(profile[label], 16384, 1):
            raise ValueError(f"Invalid scene-load profile field {label!r}")
    return profile


def parse_line(data):
    return json.loads(
        data.decode("utf-8"), object_pairs_hook=_unique_object, parse_constant=_invalid_constant,
    )


def require_timing(message):
    if not isinstance(message, str):
        raise ValueError("Scene-load log row needs a JSON message")
    event = parse_line(message.encode("utf-8"))
    if (not isinstance(event, dict) or set(event) != TIMING_FIELDS
            or not integer(event["schema_version"], 1, 1)):
        raise ValueError("Expected scene-load timing schema 1")
    request = event["request_id"]
    if not isinstance(request, str) or len(request) != 36 or str(UUID(request)) != request:
        raise ValueError("Invalid scene-load request ID")
    if (
        event["host"] not in HOSTS
        or event["scope"] not in (*SCOPES.values(), "warmup")
        or event["outcome"] not in ("presented", "cancelled", "failed")
    ):
        raise ValueError("Invalid scene-load host, scope or terminal outcome")
    elapsed = event["elapsed_us"]
    if not integer(elapsed, MAX_MICROSECONDS) or any(
        event[name] is not None and not integer(event[name], elapsed) for name in METRICS
    ):
        raise ValueError("Invalid scene-load duration")
    if event["outcome"] == "presented":
        if event["first_present_us"] != elapsed or any(event[name] is None for name in PHASES):
            raise ValueError("Presented attempt lacks complete first-frame timings")
        # Each adjacent phase is rounded down to integer microseconds independently.
        phase_sum = sum(event[name] for name in PHASES)
        if not 0 <= elapsed - phase_sum < len(PHASES):
            raise ValueError("Scene-load phases do not cover the first presentation")
    elif event["first_present_us"] is not None or event["presentation_us"] is not None:
        raise ValueError("Non-presented attempt has presentation timings")
    return event


def log_records(paths):
    """Read bounded ordinary JSONL files and retain record locations."""
    if not 1 <= len(paths) <= MAX_LOGS:
        raise ValueError(f"Provide one to {MAX_LOGS} scene-load log files")
    remaining = MAX_LOG_BYTES
    flags = os.O_RDONLY | getattr(os, "O_BINARY", 0)
    flags |= getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0)
    for path in paths:
        metadata = path.lstat()
        if not stat.S_ISREG(metadata.st_mode) or (
            getattr(metadata, "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT
        ):
            raise ValueError(f"{path.name} must be an ordinary log file")
        with os.fdopen(os.open(path, flags), "rb") as source:
            metadata = os.fstat(source.fileno())
            if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > remaining:
                raise ValueError(f"{path.name} is not a bounded ordinary log file")
            number = 0
            while line := source.readline(min(remaining, MAX_LINE_BYTES) + 1):
                number += 1
                remaining -= len(line)
                if remaining < 0 or len(line) > MAX_LINE_BYTES:
                    raise ValueError(f"Scene-load log size limit at {path.name}:{number}")
                if not line.strip():
                    continue
                try:
                    row = parse_line(line)
                    if not isinstance(row, dict):
                        raise ValueError("Expected a JSON log row")
                except (ValueError, UnicodeError, RecursionError) as error:
                    raise ValueError(f"{path.name}:{number} {error}") from error
                yield path, number, row


def timing_events(paths):
    seen = set()
    for path, number, row in log_records(paths):
        if row.get("component") != "scene_load":
            continue
        try:
            event = require_timing(row.get("message"))
            request = event["request_id"]
            if request in seen:
                raise ValueError("Duplicate terminal scene-load request")
            seen.add(request)
            if len(seen) > MAX_EVENTS:
                raise ValueError(f"Loaded more than {MAX_EVENTS} scene-load attempts")
        except (ValueError, UnicodeError, RecursionError) as error:
            raise ValueError(f"{path.name}:{number} {error}") from error
        yield event


def summarize(events, host, scopes, minimum):
    if host not in HOSTS or not integer(minimum, MAX_SAMPLES, MIN_SAMPLES):
        raise ValueError("Invalid benchmark host or sample minimum")
    by_scope = {SCOPES[scope]: [] for scope in scopes}
    for event in events:
        if event["host"] != host or event["scope"] not in by_scope:
            continue
        if event["outcome"] != "presented":
            raise ValueError(
                f"Unexpected {event['outcome']} {event['scope']} request {event['request_id']}"
            )
        samples = by_scope[event["scope"]]
        if len(samples) == MAX_SAMPLES:
            raise ValueError(f"Too many {event['scope']} scene-load samples")
        samples.append(event)
    measurements = {}
    for scope in scopes:
        samples = by_scope[SCOPES[scope]]
        count = len(samples)
        if count < minimum:
            raise ValueError(f"{scope} needs at least {minimum} attempts, found {count}")
        # Nearest rank at 95 percent, with a zero-based index.
        rank = (95 * count + 99) // 100 - 1
        stats = {"samples": count}
        for metric in METRICS:
            ordered = sorted(sample[metric] for sample in samples)
            stats[metric] = {"median": statistics.median(ordered), "p95": ordered[rank]}
        measurements[scope] = stats
    return measurements


def ceilings(measurements, headroom, jitter):
    def ceiling(p95):
        margin = max(jitter, (p95 * headroom + 99) // 100)
        return min(MAX_MICROSECONDS, p95 + margin)

    return {
        scope: {
            metric: ceiling(values[metric]["p95"])
            for metric in METRICS
        }
        for scope, values in measurements.items()
    }


def validate_budget(budget):
    required = {
        "schema_version", "profile", "fixture_sha256", "host", "scopes",
        "minimum_samples", "headroom_percent", "jitter_us", "baseline", "ceilings_us",
    }
    if (not isinstance(budget, dict) or set(budget) != required
            or not integer(budget["schema_version"], 1, 1)):
        raise ValueError("Expected scene-load budget schema 1")
    fixture_id(budget["fixture_sha256"])
    validate_profile(budget["profile"])
    scopes = budget["scopes"]
    if (not isinstance(scopes, list) or not scopes or len(scopes) > len(SCOPES)
            or any(not isinstance(scope, str) or scope not in SCOPES for scope in scopes)
            or len(set(scopes)) != len(scopes)):
        raise ValueError("Invalid scene-load budget scopes")
    if (budget["host"] not in HOSTS
            or not integer(budget["minimum_samples"], MAX_SAMPLES, MIN_SAMPLES)
            or not integer(budget["headroom_percent"], MAX_HEADROOM_PERCENT)
            or not integer(budget["jitter_us"], MAX_JITTER_US)):
        raise ValueError("Invalid scene-load budget host, sample minimum or headroom")
    baseline = budget["baseline"]
    if not isinstance(baseline, dict) or set(baseline) != set(scopes):
        raise ValueError("Invalid scene-load baseline")
    for sample in baseline.values():
        if (not isinstance(sample, dict) or set(sample) != {"samples", *METRICS}
                or not integer(sample["samples"], MAX_SAMPLES, budget["minimum_samples"])):
            raise ValueError("Invalid scene-load baseline sample count")
        for metric in METRICS:
            values = sample[metric]
            if (not isinstance(values, dict) or set(values) != {"median", "p95"}
                    or not integer(values["p95"], MAX_MICROSECONDS)
                    or type(values["median"]) not in (int, float)
                    or not 0 <= values["median"] <= MAX_MICROSECONDS):
                raise ValueError("Invalid scene-load baseline statistics")
    caps = budget["ceilings_us"]
    expected = ceilings(baseline, budget["headroom_percent"], budget["jitter_us"])
    if (caps != expected or any(type(cap) is not int for table in caps.values()
                                for cap in table.values())):
        raise ValueError("Scene-load ceilings do not match their calibration")
    return budget


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest="command", required=True)
    calibrate = subcommands.add_parser("calibrate")
    check = subcommands.add_parser("check")
    for command in (calibrate, check):
        command.add_argument("--log", type=Path, action="append", required=True)
        command.add_argument("--fixture-sha256", required=True)
        command.add_argument("--profile", type=Path, required=True)
        command.add_argument("--output", type=Path, required=True)
    calibrate.add_argument("--host", choices=HOSTS, required=True)
    calibrate.add_argument("--scope", choices=(*SCOPES, "both"), required=True)
    calibrate.add_argument("--minimum-samples", type=int, default=MIN_SAMPLES)
    calibrate.add_argument("--headroom-percent", type=int, required=True)
    calibrate.add_argument("--jitter-us", type=int, default=0)
    check.add_argument("--budget", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        digest = fixture_id(args.fixture_sha256)
        profile = validate_profile(read_local_json(args.profile, MAX_METADATA_BYTES)[0])
        if args.command == "calibrate":
            if not integer(args.headroom_percent, MAX_HEADROOM_PERCENT):
                raise ValueError("Calibration headroom must be in 0–1000 percent")
            if not integer(args.jitter_us, MAX_JITTER_US):
                raise ValueError("Calibration jitter must be in 0–1000000 microseconds")
            scopes = ["cold", "warm"] if args.scope == "both" else [args.scope]
            baseline = summarize(timing_events(args.log), args.host, scopes, args.minimum_samples)
            result = {
                "schema_version": 1,
                "fixture_sha256": digest,
                "profile": profile,
                "host": args.host,
                "scopes": scopes,
                "minimum_samples": args.minimum_samples,
                "headroom_percent": args.headroom_percent,
                "jitter_us": args.jitter_us,
                "baseline": baseline,
                "ceilings_us": ceilings(baseline, args.headroom_percent, args.jitter_us),
            }
            code = 0
        else:
            budget = validate_budget(read_local_json(args.budget, MAX_METADATA_BYTES)[0])
            if profile != budget["profile"] or digest != budget["fixture_sha256"]:
                raise ValueError("Scene-load profile or fixture does not match the calibration")
            measurements = summarize(
                timing_events(args.log), budget["host"], budget["scopes"], budget["minimum_samples"],
            )
            regressions = [
                {"scope": scope, "metric": metric, "p95_us": stats[metric]["p95"], "ceiling_us": cap}
                for scope, stats in measurements.items()
                for metric, cap in budget["ceilings_us"][scope].items()
                if stats[metric]["p95"] > cap
            ]
            result = {
                "schema_version": 1,
                "fixture_sha256": digest,
                "profile": profile,
                "host": budget["host"],
                "measurements": measurements,
                "regressions": regressions,
                "passed": not regressions,
            }
            code = int(bool(regressions))
        with args.output.open("x", encoding="utf-8", newline="\n") as output:
            json.dump(result, output, indent=2, sort_keys=True, allow_nan=False)
            output.write("\n")
    except (OSError, ValueError, UnicodeError, RecursionError) as error:
        print(f"Scene-load budget unavailable: {error}", file=sys.stderr)
        return 2
    print(f"Scene-load {args.command} {'passed' if code == 0 else 'exceeded budget'}: {args.output}")
    return code


if __name__ == "__main__":
    raise SystemExit(main())
