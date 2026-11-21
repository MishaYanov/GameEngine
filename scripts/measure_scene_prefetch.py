"""Compare the same packaged scene with CPU prefetch disabled and enabled."""

import argparse
from collections import Counter
import json
from pathlib import Path
import re
import statistics
import sys
from uuid import UUID

from compare_playtrace import read_local_json
from measure_scene_load import (
    MAX_EVENTS, MAX_METADATA_BYTES, MAX_MICROSECONDS, MAX_SAMPLES, fixture_id,
    integer, log_records, parse_line, require_timing, validate_profile,
)


MIN_TRANSITIONS = 30
FRAMES_PER_SAMPLE = 3
MAX_RETAINED_ESTIMATE_BYTES = 8 * 1024 * 1024
OUTCOMES = ("disabled", "hit", "miss", "stale")
LOAD_FIELDS = {
    "schema_version", "kind", "request_id", "scene_id", "outcome", "retained_estimate_bytes",
}
ACTIVE_FIELDS = {"schema_version", "kind", "sample", "frame", "enabled", "elapsed_us"}
SCENE_ID = re.compile(r"[a-zA-Z0-9_-]{1,64}\Z")


def measurements(values):
    ordered = sorted(values)
    return {
        "samples": len(ordered),
        "median": statistics.median(ordered),
        "p95": ordered[(95 * len(ordered) + 99) // 100 - 1],
    }


def prefetch_record(value, enabled):
    if not isinstance(value, str):
        raise ValueError("Prefetch log row needs a JSON message")
    event = parse_line(value.encode("utf-8"))
    if not isinstance(event, dict) or not integer(event.get("schema_version"), 1, 1):
        raise ValueError("Unsupported prefetch schema")
    if event.get("kind") == "load" and set(event) == LOAD_FIELDS:
        request = event["request_id"]
        if not isinstance(request, str) or len(request) != 36 or str(UUID(request)) != request:
            raise ValueError("Invalid prefetch request ID")
        scene = event["scene_id"]
        if not isinstance(scene, str) or not SCENE_ID.fullmatch(scene):
            raise ValueError("Invalid prefetched scene ID")
        outcome = event["outcome"]
        if outcome not in OUTCOMES or (outcome == "disabled") != (not enabled):
            raise ValueError("Prefetch load outcome does not match the supplied mode")
        retained = event["retained_estimate_bytes"]
        if (not integer(retained, MAX_RETAINED_ESTIMATE_BYTES)
                or (outcome in ("disabled", "miss") and retained != 0)):
            raise ValueError("Invalid retained byte estimate")
        return event
    if event.get("kind") == "active_frame" and set(event) == ACTIVE_FIELDS:
        if (not integer(event["sample"], 4096)
                or not integer(event["frame"], FRAMES_PER_SAMPLE - 1)
                or type(event["enabled"]) is not bool or event["enabled"] != enabled
                or not integer(event["elapsed_us"], MAX_MICROSECONDS)):
            raise ValueError("Invalid active-frame measurement or mode")
        return event
    raise ValueError("Unexpected prefetch record fields")


def collect_mode(paths, scene, enabled):
    loads = {}
    timings = {}
    frames = {}
    sessions = set()
    for path, line, row in log_records(paths):
        component = row.get("component")
        if component not in ("scene_prefetch", "scene_load"):
            continue
        try:
            session = row.get("session_id")
            if not isinstance(session, str) or not 1 <= len(session) <= 128 or not session.isprintable():
                raise ValueError("Timing and prefetch records require a logger session ID")
            sessions.add(session)
            if component == "scene_load":
                event = require_timing(row.get("message"))
                request = event["request_id"]
                if request in timings:
                    raise ValueError("Duplicate terminal scene-load request")
                timings[request] = (session, event)
            else:
                event = prefetch_record(row.get("message"), enabled)
                if event["kind"] == "load":
                    request = event["request_id"]
                    if request in loads:
                        raise ValueError("Duplicate prefetch load request")
                    loads[request] = (session, event)
                else:
                    group = frames.setdefault((session, event["sample"]), {})
                    if event["frame"] in group:
                        raise ValueError("Duplicate active frame in the same logger session")
                    group[event["frame"]] = event["elapsed_us"]
            if max(len(loads), len(timings), len(frames)) > MAX_EVENTS:
                raise ValueError("Too many prefetch benchmark events")
        except (ValueError, UnicodeError, RecursionError) as error:
            raise ValueError(f"{path.name}:{line} {error}") from error

    selected = []
    for request, (session, event) in loads.items():
        terminal = timings.get(request)
        if terminal is None:
            raise ValueError(f"Prefetch load {request} has no terminal timing")
        match_session, timing = terminal
        if match_session != session or timing["host"] != "player":
            raise ValueError("Prefetch load and terminal timing have different owners")
        if timing["scope"] == "play":
            if timing["outcome"] != "presented":
                raise ValueError(f"Measured player load {request} did not present")
            if event["scene_id"] == scene:
                selected.append((session, event, timing))
    for request, (_, timing) in timings.items():
        if timing["host"] == "player" and timing["scope"] == "play" and request not in loads:
            raise ValueError(f"Measured player load {request} lacks its prefetch outcome")
    if not MIN_TRANSITIONS <= len(selected) <= MAX_SAMPLES:
        raise ValueError(f"Expected {MIN_TRANSITIONS}–{MAX_SAMPLES} measured {scene} transitions")
    if len(frames) != len(selected) or any(len(frame) != FRAMES_PER_SAMPLE for frame in frames.values()):
        raise ValueError("Expected one complete three-frame active sample per selected transition")
    if (Counter(session for session, _ in frames) != Counter(session for session, _, _ in selected)
            or {session for session, _ in frames} != sessions):
        raise ValueError("Every benchmark session must contribute paired transitions and frames")
    values = [elapsed for frame in frames.values() for elapsed in frame.values()]
    return {
        "transition_first_present_us": measurements([t["first_present_us"] for _, _, t in selected]),
        "transition_cpu_us": measurements([t["cpu_us"] for _, _, t in selected]),
        "active_frame_us": measurements(values),
        "prefetch_outcomes": dict(sorted(Counter(event["outcome"] for _, event, _ in selected).items())),
        "max_retained_estimate_bytes": max(event["retained_estimate_bytes"] for _, event, _ in selected),
    }, sessions, timings.keys()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--disabled", type=Path, action="append", required=True)
    parser.add_argument("--enabled", type=Path, action="append", required=True)
    parser.add_argument("--fixture-sha256", required=True)
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--scene", default="garden")
    parser.add_argument("--active-headroom-us", type=int, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        if not SCENE_ID.fullmatch(args.scene) or not integer(args.active_headroom_us, 1_000_000):
            raise ValueError("Invalid target scene or active-frame margin")
        paths = args.disabled + args.enabled
        if len({path.resolve() for path in paths}) != len(paths):
            raise ValueError("Each log must appear once across the two modes")
        digest = fixture_id(args.fixture_sha256)
        profile = validate_profile(read_local_json(args.profile, MAX_METADATA_BYTES)[0])
        off, off_sessions, off_requests = collect_mode(args.disabled, args.scene, False)
        on, on_sessions, on_requests = collect_mode(args.enabled, args.scene, True)
        if off_sessions & on_sessions or off_requests & on_requests:
            raise ValueError("Disabled and enabled modes must use independent native runs")
        if off["transition_first_present_us"]["samples"] != on["transition_first_present_us"]["samples"]:
            raise ValueError("Disabled and enabled modes need equal measured sample counts")
        limit = off["active_frame_us"]["p95"] + args.active_headroom_us
        passed = on["active_frame_us"]["p95"] <= limit
        report = {
            "schema_version": 1,
            "scene_id": args.scene,
            "fixture_sha256": digest,
            "profile": profile,
            "active_headroom_us": args.active_headroom_us,
            "active_frame_p95_ceiling_us": limit,
            "active_frame_passed": passed,
            "disabled": off,
            "enabled": on,
        }
        with args.output.open("x", encoding="utf-8", newline="\n") as output:
            json.dump(report, output, indent=2)
            output.write("\n")
        print(f"Prefetch comparison written to {args.output}")
        return int(not passed)
    except (OSError, TypeError, ValueError, UnicodeError, RecursionError) as error:
        print(f"Cannot compare prefetch runs: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
