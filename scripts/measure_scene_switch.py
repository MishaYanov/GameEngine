"""Summarize request-keyed native A-to-B-to-A smoke runs by direction."""

import argparse
from collections import Counter, defaultdict
import json
from pathlib import Path
from statistics import median
import sys
from uuid import UUID

from compare_playtrace import read_local_json
from measure_scene_load import (
    MAX_EVENTS, MAX_METADATA_BYTES, MAX_MICROSECONDS, MAX_SAMPLES, METRICS,
    fixture_id, integer, log_records, parse_line, require_timing, validate_profile,
)


SAMPLE_FIELDS = {
    "schema_version", "request_id", "index", "source", "destination", "reason",
    "active_frame_us", "frame_gap_us", "before", "settled", "preparation",
}
PHASES = ("read_us", "simulation_us", "native_assembly_us", "drain_us", "install_us")
ASSET_METRICS = ("cache_hits", "cache_misses", "static_import_us", "static_upload_us")
RESOURCES = ("models", "cpu_retained_source_bytes", "logical_gpu_bytes", "logical_gpu_resources")
TRACKED = ("allocations", "bytes", "peak_bytes")


def unsigned_fields(data, fields):
    return (
        isinstance(data, dict)
        and set(data) == set(fields)
        and all(integer(data[key], MAX_MICROSECONDS) for key in fields)
    )


def resource_snapshot(data):
    if not isinstance(data, dict) or set(data) != {*RESOURCES, "tracked_gpu"}:
        raise ValueError("Expected scene switch resource counters")
    if not all(integer(data[key], MAX_MICROSECONDS) for key in RESOURCES):
        raise ValueError("Invalid scene switch resource counters")
    tracked = data["tracked_gpu"]
    if tracked is not None and (
        not unsigned_fields(tracked, TRACKED) or tracked["peak_bytes"] < tracked["bytes"]
    ):
        raise ValueError("Invalid tracked GPU allocation counters")


def switch_sample(message):
    data = parse_line(message.encode("utf-8"))
    if (not isinstance(data, dict) or set(data) != SAMPLE_FIELDS
            or not integer(data["schema_version"], 1, 1) or data["reason"] != "smoke_cycle"):
        raise ValueError("Expected native scene switch schema 1")
    request = data["request_id"]
    if not isinstance(request, str) or len(request) != 36 or str(UUID(request)) != request:
        raise ValueError("Invalid scene switch request ID")
    for key in ("source", "destination"):
        scene = data[key]
        if (not isinstance(scene, str) or not 1 <= len(scene.encode("utf-8")) <= 256
                or any(char.isspace() or ord(char) < 32 or 127 <= ord(char) <= 159
                       for char in scene)):
            raise ValueError(f"Invalid scene switch {key} ID")
    if data["source"] == data["destination"] or not integer(data["index"], MAX_SAMPLES + 1):
        raise ValueError("Invalid scene switch direction or index")
    frames = data["active_frame_us"]
    if (not isinstance(frames, list) or len(frames) != 3
            or not all(integer(frame, MAX_MICROSECONDS) for frame in frames)):
        raise ValueError("Expected three source frame samples for a scene switch")
    preparation = data["preparation"]
    if not isinstance(preparation, dict) or set(preparation) != {*PHASES, "assets", "overlap"}:
        raise ValueError("Invalid scene switch preparation")
    if any(preparation[key] is not None and not integer(preparation[key], MAX_MICROSECONDS)
           for key in PHASES):
        raise ValueError("Invalid preparation phase duration")
    assets = preparation["assets"]
    if assets is not None and not unsigned_fields(assets, ASSET_METRICS):
        raise ValueError("Invalid static asset phase counters")
    if data["frame_gap_us"] is not None and not integer(data["frame_gap_us"], MAX_MICROSECONDS):
        raise ValueError("Invalid scene switch frame gap")
    resource_snapshot(data["before"])
    for resource in (preparation["overlap"], data["settled"]):
        if resource is not None:
            resource_snapshot(resource)
    return data


def summary(values):
    samples = sorted(values)
    count = len(samples)
    if not count:
        return None
    return {
        "samples": count,
        "median": median(samples),
        "p95": samples[(95 * count + 99) // 100 - 1],
        "max": samples[-1],
    }


def resource_summary(snapshots):
    tracked = [snapshot["tracked_gpu"] for snapshot in snapshots if snapshot["tracked_gpu"] is not None]
    return {
        **{key: summary(snapshot[key] for snapshot in snapshots) for key in RESOURCES},
        "tracked_gpu": {key: summary(snapshot[key] for snapshot in tracked) for key in TRACKED},
    }


def direction_summary(source, destination, rows, frame_budget_us):
    presented = [(sample, timing) for sample, timing in rows if timing["outcome"] == "presented"]
    frames = [frame for sample, _ in rows for frame in sample["active_frame_us"]]
    gaps = [sample["frame_gap_us"] for sample, _ in presented]
    return {
        "source": source,
        "destination": destination,
        "attempts": len(rows),
        "outcomes": dict(sorted(Counter(timing["outcome"] for _, timing in rows).items())),
        "timing": {key: summary(timing[key] for _, timing in presented) for key in METRICS},
        "preparation": {
            key: summary(sample["preparation"][key] for sample, _ in presented) for key in PHASES
        },
        "static_assets": {
            key: summary(sample["preparation"]["assets"][key] for sample, _ in presented)
            for key in ASSET_METRICS
        },
        "source_frame_us": summary(frames),
        "source_frames_over_budget": sum(frame > frame_budget_us for frame in frames),
        "frame_gap_us": summary(gaps),
        "switch_gaps_over_budget": sum(gap > frame_budget_us for gap in gaps),
        "resources": {
            "before": resource_summary([sample["before"] for sample, _ in rows]),
            "overlap": resource_summary([sample["preparation"]["overlap"] for sample, _ in presented]),
            "settled": resource_summary([sample["settled"] for sample, _ in presented]),
        },
    }


def validate_companion(sample, timing):
    presented = timing["outcome"] == "presented"
    if timing["host"] != "player" or timing["scope"] != ("warmup" if sample["index"] < 2 else "play"):
        raise ValueError("Scene switch request has the wrong terminal scope or owner")
    if not presented:
        if sample["settled"] is not None or sample["frame_gap_us"] is not None:
            raise ValueError("An unsuccessful switch has presented evidence")
        return
    prep = sample["preparation"]
    if (sample["settled"] is None or sample["frame_gap_us"] is None
            or prep["overlap"] is None or prep["assets"] is None
            or any(prep[key] is None for key in PHASES)):
        raise ValueError("A presented scene switch is missing measurements")
    if (sum(prep[key] for key in PHASES) > timing["elapsed_us"]
            or sample["frame_gap_us"] < timing["first_present_us"]
            or sum(prep["assets"][key] for key in ("static_import_us", "static_upload_us"))
            > prep["native_assembly_us"]):
        raise ValueError("Scene switch phase durations contradict the terminal timing")


def collect_runs(paths, minimum, frame_budget_us):
    timings = {}
    switches = {}
    for path, line, row in log_records(paths):
        target = row.get("component")
        if target not in ("scene_load", "scene_switch"):
            continue
        try:
            session = row.get("session_id")
            if not isinstance(session, str) or not 1 <= len(session) <= 128 or not session.isprintable():
                raise ValueError("Scene switch records require a logger session")
            message = row.get("message")
            if not isinstance(message, str):
                raise ValueError("Expected a scene switch JSON message")
            event = require_timing(message) if target == "scene_load" else switch_sample(message)
            events = timings if target == "scene_load" else switches
            request = event["request_id"]
            if request in events:
                raise ValueError(f"Duplicate {target} request")
            if len(events) >= MAX_EVENTS:
                raise ValueError("Too many native scene switch events")
            events[request] = (session, event)
        except (ValueError, UnicodeError, RecursionError) as error:
            raise ValueError(f"{path.name}:{line} {error}") from error

    by_session = defaultdict(list)
    for request, (session, sample) in switches.items():
        terminal = timings.get(request)
        if terminal is None or terminal[0] != session:
            raise ValueError(f"Scene switch {request} has no matching terminal timing")
        timing = terminal[1]
        validate_companion(sample, timing)
        by_session[session].append((sample, timing))
    if not by_session:
        raise ValueError("No native scene switch samples found")

    startup = defaultdict(list)
    for request, (session, event) in timings.items():
        if session not in by_session or event["host"] != "player":
            continue
        if event["scope"] == "startup" and event["outcome"] == "presented":
            startup[session].append(event["first_present_us"])
        if event["scope"] in ("play", "warmup") and request not in switches:
            raise ValueError(f"Unpaired benchmark terminal request {request}")

    runs = []
    for session, rows in sorted(by_session.items()):
        rows.sort(key=lambda row: row[0]["index"])
        if len(rows) < 2 or [sample["index"] for sample, _ in rows] != list(range(len(rows))):
            raise ValueError("Expected contiguous smoke cycle indices from both warmup switches")
        first = rows[0][0]
        directions = [(first["source"], first["destination"]), (first["destination"], first["source"])]
        for sample, timing in rows:
            index = sample["index"]
            if (sample["source"], sample["destination"]) != directions[index % 2]:
                raise ValueError("Expected alternating A to B and B to A directions")
            if index < 2 and timing["outcome"] != "presented":
                raise ValueError("The native scene switch warmup did not present")
        measurements = [
            direction_summary(source, destination, rows[2 + offset::2], frame_budget_us)
            for offset, (source, destination) in enumerate(directions)
        ]
        runs.append({
            "session_id": session,
            "startup_first_present_us": summary(startup[session]),
            "directions": measurements,
            "passed": all(
                direction["attempts"] >= minimum
                and direction["outcomes"].get("presented", 0) == direction["attempts"]
                for direction in measurements
            ),
        })
    return runs


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--log", type=Path, action="append", required=True)
    parser.add_argument("--fixture-sha256", required=True)
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--minimum-samples", type=int, default=30, help="measured switches per direction and run")
    parser.add_argument("--frame-budget-us", type=int, default=16_667)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        digest = fixture_id(args.fixture_sha256)
        profile = validate_profile(read_local_json(args.profile, MAX_METADATA_BYTES)[0])
        if (not integer(args.minimum_samples, MAX_SAMPLES, 1)
                or not integer(args.frame_budget_us, 1_000_000, 1)):
            raise ValueError("Invalid sample minimum or source frame budget")
        runs = collect_runs(args.log, args.minimum_samples, args.frame_budget_us)
        passed = all(run["passed"] for run in runs)
        report = {
            "schema_version": 1,
            "fixture_sha256": digest,
            "profile": profile,
            "minimum_samples_per_direction": args.minimum_samples,
            "frame_budget_us": args.frame_budget_us,
            "runs": runs,
            "passed": passed,
        }
        with args.output.open("x", encoding="utf-8", newline="\n") as stream:
            json.dump(report, stream, indent=2, allow_nan=False)
            stream.write("\n")
    except (OSError, ValueError, UnicodeError, RecursionError) as error:
        print(f"Cannot summarize scene switches: {error}", file=sys.stderr)
        return 2
    print(f"Scene switch report written to {args.output}")
    return int(not passed)


if __name__ == "__main__":
    raise SystemExit(main())
