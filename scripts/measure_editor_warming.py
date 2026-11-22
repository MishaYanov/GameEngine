"""Compare native editor scene warming on one fixture, scene and viewport profile."""

import argparse
from collections import Counter
import json
from pathlib import Path
import sys
from unicodedata import category
from uuid import UUID

from compare_playtrace import read_local_json
from measure_scene_load import (
    MAX_EVENTS, MAX_METADATA_BYTES, MAX_MICROSECONDS, MAX_SAMPLES, fixture_id,
    integer, log_records, parse_line, require_timing, validate_profile,
)
from measure_scene_prefetch import measurements


MIN_SAMPLES = 30
WARM_OUTCOMES = ("unrequested", "hit", "miss", "stale")
TERMINAL_OUTCOMES = ("presented", "cancelled", "failed")
COMMON_FIELDS = {"schema_version", "kind", "scene_id"}
LOAD_FIELDS = COMMON_FIELDS | {"request_id", "outcome"}
FRAME_FIELDS = COMMON_FIELDS | {"warming_requested", "elapsed_us"}
MODEL_FIELDS = COMMON_FIELDS | {
    "request_id", "mode", "batch_matched", "prepared_models", "consumed_models", "retained_bytes",
}
MODEL_COMPONENT = "editor_model_warming"
MAX_WARM_MODELS = 8
MAX_RETAINED_MODEL_BYTES = 16 * 1024 * 1024


def scene_id(value):
    if (not isinstance(value, str) or not 1 <= len(value.encode("utf-8")) <= 256
            or any(char.isspace() or category(char) == "Cc" for char in value)):
        raise ValueError("Invalid authored scene ID")
    return value


def request_id(value):
    if not isinstance(value, str) or len(value) != 36 or str(UUID(value)) != value:
        raise ValueError("Invalid editor warming request ID")
    return value


def warming_record(message):
    if not isinstance(message, str):
        raise ValueError("Editor warming log row needs a JSON message")
    event = parse_line(message.encode("utf-8"))
    if not isinstance(event, dict) or not integer(event.get("schema_version"), 1, 1):
        raise ValueError("Expected editor warming schema 1")
    if event.get("kind") == "load" and set(event) == LOAD_FIELDS:
        request_id(event["request_id"])
        if event["outcome"] not in WARM_OUTCOMES:
            raise ValueError("Invalid editor warming outcome")
    elif event.get("kind") == "active_frame" and set(event) == FRAME_FIELDS:
        if (type(event["warming_requested"]) is not bool
                or not integer(event["elapsed_us"], MAX_MICROSECONDS)):
            raise ValueError("Invalid active-frame measurement")
    else:
        raise ValueError("Unexpected editor warming record fields")
    scene_id(event["scene_id"])
    return event


def model_record(message):
    if not isinstance(message, str):
        raise ValueError("Editor model warming log row needs a JSON message")
    event = parse_line(message.encode("utf-8"))
    if (not isinstance(event, dict) or set(event) != MODEL_FIELDS
            or not integer(event["schema_version"], 1, 1) or event["kind"] != "model_load"):
        raise ValueError("Invalid editor model warming record")
    request_id(event["request_id"])
    scene_id(event["scene_id"])
    prepared = event["prepared_models"]
    consumed = event["consumed_models"]
    retained = event["retained_bytes"]
    matched = event["batch_matched"]
    if (event["mode"] not in ("off", "references", "decode") or type(matched) is not bool
            or not integer(prepared, MAX_WARM_MODELS) or not integer(consumed, prepared)
            or not integer(retained, MAX_RETAINED_MODEL_BYTES)):
        raise ValueError("Invalid editor model count or retained-byte budget")
    if ((matched and (event["mode"] != "decode" or not prepared or not retained))
            or (not matched and (prepared or consumed or retained))):
        raise ValueError("Editor model counts require a matching decode batch")
    return event


def record_request(requests, session, event):
    key = session, event["request_id"]
    if key in requests:
        raise ValueError(f"Duplicate {event.get('kind', 'terminal')} record for request {key[1]}")
    if len(requests) == MAX_EVENTS:
        raise ValueError("Too many editor load records")
    requests[key] = event


def collect_mode(paths, target_scene, active_scene, warmed, decode_evidence=False):
    loads = {}
    timings = {}
    models = {}
    frames = []
    sessions = set()
    active_sessions = set()

    for path, number, row in log_records(paths):
        component = row.get("component")
        if component not in ("scene_load", "editor_scene_warming"):
            if not decode_evidence or component != MODEL_COMPONENT:
                continue
        try:
            if component == "scene_load":
                event = require_timing(row.get("message"))
                if event["host"] != "editor" or event["scope"] != "authored":
                    continue
            elif component == MODEL_COMPONENT:
                event = model_record(row.get("message"))
            else:
                event = warming_record(row.get("message"))
            session = row.get("session_id")
            if not isinstance(session, str) or not 1 <= len(session) <= 128 or not session.isprintable():
                raise ValueError("Editor benchmark records require a logger session ID")
            sessions.add(session)
            if len(sessions) > MAX_EVENTS:
                raise ValueError("Too many editor benchmark sessions")

            if component == "scene_load":
                record_request(timings, session, event)
            elif component == MODEL_COMPONENT:
                record_request(models, session, event)
            elif event["kind"] == "load":
                record_request(loads, session, event)
            elif event["scene_id"] == active_scene:
                if not warmed and event["warming_requested"]:
                    raise ValueError("Baseline active frames include a warming request")
                if event["warming_requested"] == warmed:
                    if len(frames) == MAX_EVENTS:
                        raise ValueError("Too many measured active frames")
                    frames.append(event["elapsed_us"])
                    active_sessions.add(session)
        except (ValueError, UnicodeError, RecursionError) as error:
            raise ValueError(f"{path.name}:{number} {error}") from error

    if loads.keys() != timings.keys():
        raise ValueError("Every editor warming load needs one authored terminal in the same session")
    if decode_evidence:
        if loads.keys() != models.keys():
            raise ValueError("Every editor warming load needs one model record in the same session")
        for key, record in models.items():
            load = loads[key]
            expected_mode = "decode" if warmed and load["outcome"] != "unrequested" else "off"
            if record["scene_id"] != load["scene_id"] or record["mode"] != expected_mode:
                raise ValueError("Editor model record does not match its scene or capture mode")
            if record["batch_matched"] and load["outcome"] != "hit":
                raise ValueError("A matching model batch requires a retained-reference hit")
    selected = [(key, load, timings[key]) for key, load in loads.items()
                if load["scene_id"] == target_scene]
    presented = [(key, timing) for key, _, timing in selected if timing["outcome"] == "presented"]
    if not MIN_SAMPLES <= len(presented) <= MAX_SAMPLES:
        raise ValueError(f"Expected {MIN_SAMPLES} to {MAX_SAMPLES} presented {target_scene} loads")
    if len(frames) < MIN_SAMPLES:
        raise ValueError(f"Expected at least {MIN_SAMPLES} eligible {active_scene} active frames")
    if active_sessions != {key[0] for key, _ in presented}:
        raise ValueError("Presented target loads and measured active frames must share the same sessions")

    warming_outcomes = Counter(load["outcome"] for _, load, _ in selected)
    if not warmed and len(selected) != warming_outcomes["unrequested"]:
        raise ValueError("Baseline target loads include a warming request")
    presentation_outcomes = Counter(timing["outcome"] for _, _, timing in selected)
    result = {
        "first_present_us": measurements([timing["first_present_us"] for _, timing in presented]),
        "active_frame_us": measurements(frames),
        "warming_outcomes": {outcome: warming_outcomes[outcome] for outcome in WARM_OUTCOMES},
        "presentation_outcomes": {outcome: presentation_outcomes[outcome] for outcome in TERMINAL_OUTCOMES},
    }
    if decode_evidence:
        samples = [models[key] for key, _ in presented]
        result["model_warming"] = {
            "matched_batches": sum(event["batch_matched"] for event in samples),
            "prepared_models": sum(event["prepared_models"] for event in samples),
            "consumed_models": sum(event["consumed_models"] for event in samples),
            "max_retained_bytes": max(event["retained_bytes"] for event in samples),
        }
    return result, sessions, {request for _, request in loads}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, action="append", required=True, help="Baseline JSONL log")
    parser.add_argument("--warmed", type=Path, action="append", required=True, help="Warmed JSONL log")
    parser.add_argument("--scene", required=True, help="Target authored scene ID")
    parser.add_argument("--active-scene", help="Scene ID to sample while active (defaults to target)")
    parser.add_argument("--fixture-sha256", required=True)
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--active-ceiling-us", type=int, required=True, help="Maximum warmed active-frame p95")
    parser.add_argument("--decode-evidence", action="store_true",
                        help="Require matched model measurements from baseline and decode captures")
    parser.add_argument("--output", type=Path, required=True, help="New report JSON path")
    args = parser.parse_args(argv)
    try:
        target = scene_id(args.scene)
        active = target if args.active_scene is None else scene_id(args.active_scene)
        if not integer(args.active_ceiling_us, MAX_MICROSECONDS, 1):
            raise ValueError("Active-frame ceiling must be positive microseconds")
        paths = args.baseline + args.warmed
        if len({path.resolve() for path in paths}) != len(paths):
            raise ValueError("Each log must appear once across the two modes")
        digest = fixture_id(args.fixture_sha256)
        profile = validate_profile(read_local_json(args.profile, MAX_METADATA_BYTES)[0])
        off, off_sessions, off_requests = collect_mode(
            args.baseline, target, active, False, args.decode_evidence)
        on, on_sessions, on_requests = collect_mode(
            args.warmed, target, active, True, args.decode_evidence)
        if off_sessions & on_sessions or off_requests & on_requests:
            raise ValueError("Baseline and warmed logs must use independent native runs")
        if off["first_present_us"]["samples"] != on["first_present_us"]["samples"]:
            raise ValueError("Baseline and warmed runs need equal presented target load counts")

        passed = on["active_frame_us"]["p95"] <= args.active_ceiling_us
        report = {
            "schema_version": 1,
            "fixture_sha256": digest,
            "profile": profile,
            "scene_id": target,
            "active_scene_id": active,
            "active_frame_p95_ceiling_us": args.active_ceiling_us,
            "active_frame_passed": passed,
            "baseline": off,
            "warmed": on,
        }
        with args.output.open("x", encoding="utf-8", newline="\n") as output:
            json.dump(report, output, indent=2)
            output.write("\n")
        print(f"Editor warming comparison written to {args.output}")
        return int(not passed)
    except (OSError, TypeError, ValueError, UnicodeError, RecursionError) as error:
        print(f"Cannot compare editor warming runs: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
