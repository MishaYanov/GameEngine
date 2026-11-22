"""Compare native editor scene warming on one fixture, scene and viewport profile."""

import argparse
from collections import Counter, defaultdict
import json
from pathlib import Path
import sys
from unicodedata import category
from uuid import UUID

from compare_playtrace import read_local_json
from measure_scene_load import (
    MAX_EVENTS, MAX_LOGS, MAX_METADATA_BYTES, MAX_MICROSECONDS, MAX_SAMPLES, fixture_id,
    integer, log_records, parse_line, require_timing, validate_profile,
)
from measure_scene_prefetch import measurements


MIN_SAMPLES = 30
WARM_OUTCOMES = ("unrequested", "hit", "miss", "stale")
TERMINAL_OUTCOMES = ("presented", "cancelled", "failed")
COMMON_FIELDS = {"schema_version", "kind", "scene_id"}
LOAD_FIELDS = COMMON_FIELDS | {"request_id", "outcome"}
FRAME_FIELDS = COMMON_FIELDS | {"warming_requested", "elapsed_us"}
FRAME_IDENTITY_FIELDS = {"mode", "pre_switch_cache", "viewport", "workspace_generation"}
MODEL_FIELDS = COMMON_FIELDS | {
    "request_id", "mode", "batch_matched", "prepared_models", "consumed_models", "retained_bytes",
}
MODEL_COMPONENT = "editor_model_warming"
SESSION_COMPONENT = "editor_warming_session"
MODEL_MODES = {"off": "off", "references": "references", "models": "decode"}
MAX_WARM_MODELS = 8
MAX_RETAINED_MODEL_BYTES = 16 * 1024 * 1024
MAX_REPORT_BYTES = 64 * 1024
MAX_REPORT_ROWS = 32
IDENTITY_FIELDS = {"source_scene_id", "workspace_generation", "pre_switch_cache", "viewport"}
ROW_FIELDS = IDENTITY_FIELDS | {
    "request_id", "scene_id", "scene_revision", "mode", "mode_revision", "outcome",
    "references_hit", "prepared_models", "adopted_models", "retained_bytes",
}


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
    if not isinstance(event, dict) or not integer(event.get("schema_version"), 2, 1):
        raise ValueError("Expected editor warming schema 1 or 2")
    legacy = event["schema_version"] == 1
    frame_fields = FRAME_FIELDS if legacy else FRAME_FIELDS | FRAME_IDENTITY_FIELDS
    if legacy and event.get("kind") == "load" and set(event) == LOAD_FIELDS:
        request_id(event["request_id"])
        if event["outcome"] not in WARM_OUTCOMES:
            raise ValueError("Invalid editor warming outcome")
    elif event.get("kind") == "active_frame" and set(event) == frame_fields:
        if (type(event["warming_requested"]) is not bool
                or not integer(event["elapsed_us"], MAX_MICROSECONDS)):
            raise ValueError("Invalid active-frame measurement")
        if not legacy:
            validate_capture_fields(event)
            if event["mode"] == "off" and event["warming_requested"]:
                raise ValueError("Off active frames cannot include a warming request")
    else:
        raise ValueError("Unexpected editor warming record fields")
    scene_id(event["scene_id"])
    return event


def model_record(message):
    if not isinstance(message, str):
        raise ValueError("Editor model warming log row needs a JSON message")
    event = parse_line(message.encode("utf-8"))
    if (not isinstance(event, dict) or not integer(event.get("schema_version"), 2, 1)
            or event.get("kind") != "model_load"):
        raise ValueError("Invalid editor model warming record")
    fields = MODEL_FIELDS if event["schema_version"] == 1 else MODEL_FIELDS | {"configured_mode"}
    if set(event) != fields:
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
    if event["schema_version"] == 2:
        configured = event["configured_mode"]
        if (not isinstance(configured, str) or configured not in MODEL_MODES
                or event["mode"] not in ("off", MODEL_MODES[configured])):
            raise ValueError("Editor model record does not match its configured mode")
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


def logger_session(value):
    if not isinstance(value, str) or not 1 <= len(value) <= 128 or not value.isprintable():
        raise ValueError("Editor benchmark records require a logger session ID")
    return value


def validate_capture_fields(fields):
    if not integer(fields["workspace_generation"], MAX_MICROSECONDS, 1):
        raise ValueError("Invalid warming workspace generation")
    cache = fields["pre_switch_cache"]
    counts = {"asset_generation", "loaded_models", "gpu_resources", "gpu_bytes"}
    if (not isinstance(cache, dict) or set(cache) != counts | {"state"}
            or cache["state"] not in ("fresh", "retained", "invalidated")
            or not all(integer(cache[name], MAX_MICROSECONDS) for name in counts)
            or cache["state"] == "fresh" and any(cache[name] for name in counts - {"asset_generation"})):
        raise ValueError("Invalid native pre-switch cache condition")
    viewport = fields["viewport"]
    if (not isinstance(viewport, dict)
            or set(viewport) != {"width", "height", "projection", "grid", "colliders"}
            or not all(integer(viewport[name], 16384) for name in ("width", "height"))
            or viewport["projection"] not in ("perspective", "orthographic")
            or not all(type(viewport[name]) is bool for name in ("grid", "colliders"))):
        raise ValueError("Invalid native viewport profile")
    if "mode" in fields and (not isinstance(fields["mode"], str) or fields["mode"] not in MODEL_MODES):
        raise ValueError("Invalid native warming mode")


def session_identity_record(message):
    if not isinstance(message, str):
        raise ValueError("Editor warming session log row needs a JSON message")
    event = parse_line(message.encode("utf-8"))
    if (not isinstance(event, dict)
            or set(event) != {"schema_version", "request_id", "scene_id", "scene_revision",
                              "mode", "mode_revision", "identity"}
            or not integer(event["schema_version"], 1, 1)
            or not integer(event["scene_revision"], MAX_MICROSECONDS)
            or not integer(event["mode_revision"], MAX_MICROSECONDS)
            or not isinstance(event["mode"], str) or event["mode"] not in MODEL_MODES
            or not isinstance(event["identity"], dict) or set(event["identity"]) != IDENTITY_FIELDS):
        raise ValueError("Invalid editor warming session record")
    request_id(event["request_id"])
    scene_id(event["scene_id"])
    identity = event["identity"]
    validate_capture_fields(identity)
    if identity["source_scene_id"] is not None:
        scene_id(identity["source_scene_id"])
    return event


def read_captures(inputs, digest, profile):
    if len(inputs) > MAX_LOGS:
        raise ValueError("Too many warming capture reports")
    captures = {}
    for report_path, run_digest, profile_path in inputs:
        if fixture_id(run_digest) != digest:
            raise ValueError("Each warming capture must use the same fixture SHA-256")
        if validate_profile(read_local_json(Path(profile_path), MAX_METADATA_BYTES)[0]) != profile:
            raise ValueError("Each warming capture must use the same viewport profile")
        report, _ = read_local_json(Path(report_path), MAX_REPORT_BYTES)
        if (not isinstance(report, dict)
                or set(report) != {"schema_version", "session_id", "workspace_generation", "rows"}
                or not integer(report["schema_version"], 1, 1)
                or not integer(report["workspace_generation"], MAX_MICROSECONDS, 1)
                or not isinstance(report["rows"], list) or not 1 <= len(report["rows"]) <= MAX_REPORT_ROWS):
            raise ValueError("Expected a schema 1 warming report with 1 to 32 rows")
        session = logger_session(report["session_id"])
        if session in captures:
            raise ValueError("Provide one warming report per native logger session")
        rows = {}
        for row in report["rows"]:
            if (not isinstance(row, dict) or set(row) != ROW_FIELDS
                    or not integer(row["workspace_generation"], MAX_MICROSECONDS, 1)
                    or row["workspace_generation"] != report["workspace_generation"]
                    or not integer(row["scene_revision"], MAX_MICROSECONDS)
                    or not integer(row["mode_revision"], MAX_MICROSECONDS)
                    or row["outcome"] not in WARM_OUTCOMES
                    or type(row["references_hit"]) is not bool
                    or not integer(row["prepared_models"], MAX_WARM_MODELS)
                    or not integer(row["adopted_models"], row["prepared_models"])
                    or not integer(row["retained_bytes"], MAX_RETAINED_MODEL_BYTES)):
                raise ValueError("Invalid editor warming report row")
            validate_capture_fields(row)
            scene_id(row["scene_id"])
            if row["source_scene_id"] is not None:
                scene_id(row["source_scene_id"])
            record_request(rows, session, row | {"request_id": request_id(row["request_id"])})
        captures[session] = report["workspace_generation"], rows
    return captures


def capture_key(event):
    return tuple(sorted(event["pre_switch_cache"].items())), tuple(sorted(event["viewport"].items()))


def model_totals(samples):
    return {
        "matched_batches": sum(event["batch_matched"] for event in samples),
        "prepared_models": sum(event["prepared_models"] for event in samples),
        "consumed_models": sum(event["consumed_models"] for event in samples),
        "max_retained_bytes": max((event["retained_bytes"] for event in samples), default=0),
    }


def matched_captures(captures, extent, target, source, mode, loads, timings, models, identities, frames):
    if identities.keys() != loads.keys() or set(captures) != {session for session, _ in loads}:
        raise ValueError("Every measured native session needs a report and a complete load identity log")
    groups = defaultdict(lambda: {"loads": [], "presented": [], "models": [], "frames": []})
    for session, (generation, rows) in captures.items():
        session_groups = set()
        for key, row in rows.items():
            if key not in identities or timings[key]["outcome"] != "presented":
                raise ValueError("Every warming report row needs one presented terminal in its own native session")
            identity = identities[key]
            load = loads[key]
            model = models[key]
            if (any(row[field] != identity[field]
                    for field in ("scene_id", "scene_revision", "mode", "mode_revision"))
                    or any(row[field] != identity["identity"][field] for field in IDENTITY_FIELDS - {"viewport"})
                    or row["scene_id"] != load["scene_id"]
                    or row["outcome"] != load["outcome"]
                    or row["references_hit"] != (load["outcome"] == "hit")
                    or row["prepared_models"] != model["prepared_models"]
                    or row["adopted_models"] != model["consumed_models"]
                    or row["retained_bytes"] != model["retained_bytes"]):
                raise ValueError("Warming report identity or adoption counts disagree with the native log")
        for key, evidence in identities.items():
            if key[0] != session:
                continue
            identity = evidence["identity"]
            if (identity["workspace_generation"] != generation or identity["source_scene_id"] != source
                    or evidence["scene_id"] != target):
                continue
            load = loads[key]
            model = models[key]
            if (evidence["mode"] != mode or model.get("configured_mode") != mode
                    or load["scene_id"] != target or mode == "off" and load["outcome"] != "unrequested"):
                raise ValueError("The reported capture mode or scene disagrees with the native target load")
            viewport = identity["viewport"]
            if (viewport["width"], viewport["height"]) != extent:
                raise ValueError("The native drawable size does not match the supplied viewport profile")
            key_fields = capture_key(identity)
            session_groups.add(key_fields)
            group = groups[key_fields]
            timing = timings[key]
            group["loads"].append((load["outcome"], timing["outcome"]))
            if timing["outcome"] == "presented":
                if key not in rows or rows[key]["viewport"] != viewport:
                    raise ValueError("A presented switch is missing its report row or its viewport changed")
                group["presented"].append(timing["first_present_us"])
                group["models"].append(model)
        for frame_session, frame in frames:
            if (frame_session != session or frame.get("workspace_generation") != generation
                    or frame.get("mode") != mode):
                continue
            key = capture_key(frame)
            if key in session_groups:
                groups[key]["frames"].append(frame["elapsed_us"])
    if not groups:
        raise ValueError(f"Expected native warming measurements from {source} to {target}")
    return groups


def capture_metrics(group):
    outcomes, terminals = (Counter(events) for events in zip(*group["loads"]))
    return {
        "presented_samples": len(group["presented"]),
        "active_frame_samples": len(group["frames"]),
        "warming_outcomes": {outcome: outcomes[outcome] for outcome in WARM_OUTCOMES},
        "presentation_outcomes": {outcome: terminals[outcome] for outcome in TERMINAL_OUTCOMES},
        "model_warming": model_totals(group["models"]),
    }


def compare_capture_groups(baseline, warmed, warmed_mode, active_ceiling):
    if baseline.keys() != warmed.keys():
        raise ValueError("Off and warmed runs need matching native cache conditions and viewport settings")
    groups = []
    for key in sorted(baseline):
        cache, viewport = key
        off_group, on_group = baseline[key], warmed[key]
        if len(off_group["presented"]) != len(on_group["presented"]):
            raise ValueError("Each matched cache condition needs equal presented target load counts")
        if len(off_group["presented"]) > MAX_SAMPLES:
            raise ValueError(f"A matched cache condition supports up to {MAX_SAMPLES} presented loads per mode")
        off, on = capture_metrics(off_group), capture_metrics(on_group)
        sufficient = min(off["presented_samples"], off["active_frame_samples"], on["active_frame_samples"]) >= MIN_SAMPLES
        passed = None
        if sufficient:
            for metrics, samples in ((off, off_group), (on, on_group)):
                metrics["first_present_us"] = measurements(samples["presented"])
                metrics["active_frame_us"] = measurements(samples["frames"])
            passed = on["active_frame_us"]["p95"] <= active_ceiling
        groups.append({
            "pre_switch_cache": dict(cache), "viewport": dict(viewport),
            "status": "measured" if sufficient else "insufficient_matched_samples",
            "active_frame_passed": passed,
            "baseline": off | {"mode": "off"},
            "warmed": on | {"mode": warmed_mode},
        })
    return groups


def collect_mode(paths, target_scene, active_scene, warmed, model_mode=None, captures=None, extent=None):
    loads = {}
    timings = {}
    models = {}
    identities = {}
    frames = []
    sessions = set()
    active_sessions = set()

    for path, number, row in log_records(paths):
        component = row.get("component")
        if component not in ("scene_load", "editor_scene_warming") and not (
            captures is not None and component == SESSION_COMPONENT
        ):
            if model_mode is None or component != MODEL_COMPONENT:
                continue
        try:
            if component == "scene_load":
                event = require_timing(row.get("message"))
                if event["host"] != "editor" or event["scope"] != "authored":
                    continue
            elif component == MODEL_COMPONENT:
                event = model_record(row.get("message"))
            elif component == SESSION_COMPONENT:
                event = session_identity_record(row.get("message"))
            else:
                event = warming_record(row.get("message"))
            session = logger_session(row.get("session_id"))
            sessions.add(session)
            if len(sessions) > MAX_EVENTS:
                raise ValueError("Too many editor benchmark sessions")

            if component == "scene_load":
                record_request(timings, session, event)
            elif component == MODEL_COMPONENT:
                record_request(models, session, event)
            elif component == SESSION_COMPONENT:
                record_request(identities, session, event)
            elif event["kind"] == "load":
                record_request(loads, session, event)
            elif event["scene_id"] == active_scene:
                if captures is None and not warmed and event["warming_requested"]:
                    raise ValueError("Baseline active frames include a warming request")
                if event["warming_requested"] == warmed:
                    if len(frames) == MAX_EVENTS:
                        raise ValueError("Too many measured active frames")
                    frames.append((session, event))
                    active_sessions.add(session)
        except (ValueError, UnicodeError, RecursionError) as error:
            raise ValueError(f"{path.name}:{number} {error}") from error

    if loads.keys() != timings.keys():
        raise ValueError("Every editor warming load needs one authored terminal in the same session")
    if model_mode is not None:
        if loads.keys() != models.keys():
            raise ValueError("Every editor warming load needs one model record in the same session")
        for key, record in models.items():
            load = loads[key]
            requested = warmed and load["outcome"] != "unrequested"
            expected_mode = MODEL_MODES[model_mode] if requested else "off"
            if record["scene_id"] != load["scene_id"] or record["mode"] != expected_mode:
                raise ValueError("Editor model record does not match its scene or capture mode")
            if not warmed and record.get("configured_mode", "off") != "off":
                raise ValueError("Baseline capture needs scene warming configured Off")
            if (captures is None and warmed and load["scene_id"] == target_scene and "configured_mode" in record
                    and record["configured_mode"] != model_mode):
                raise ValueError("Warmed target loads must use the configured capture mode")
            if record["batch_matched"] and load["outcome"] != "hit":
                raise ValueError("A matching model batch requires a retained-reference hit")
    if captures is not None:
        groups = matched_captures(
            captures, extent, target_scene, active_scene, model_mode if warmed else "off",
            loads, timings, models, identities, frames,
        )
        return groups, sessions, {request for _, request in loads}
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
        "active_frame_us": measurements([event["elapsed_us"] for _, event in frames]),
        "warming_outcomes": {outcome: warming_outcomes[outcome] for outcome in WARM_OUTCOMES},
        "presentation_outcomes": {outcome: presentation_outcomes[outcome] for outcome in TERMINAL_OUTCOMES},
    }
    if model_mode is not None:
        result["model_warming"] = model_totals([models[key] for key, _ in presented])
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
                        help="Compatibility alias for --warmed-mode models")
    parser.add_argument("--warmed-mode", choices=("references", "models"),
                        help="Require matched model records for the selected warming mode")
    parser.add_argument("--baseline-capture", nargs=3, action="append",
                        metavar=("REPORT", "FIXTURE_SHA256", "PROFILE"),
                        help="Baseline session report and its independently recorded fixture digest and profile")
    parser.add_argument("--warmed-capture", nargs=3, action="append",
                        metavar=("REPORT", "FIXTURE_SHA256", "PROFILE"),
                        help="Warmed session report and its independently recorded fixture digest and profile")
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
        if args.decode_evidence and args.warmed_mode == "references":
            raise ValueError("--decode-evidence requires Models warming")
        model_mode = args.warmed_mode or ("models" if args.decode_evidence else None)
        digest = fixture_id(args.fixture_sha256)
        profile = validate_profile(read_local_json(args.profile, MAX_METADATA_BYTES)[0])
        matched = args.baseline_capture is not None or args.warmed_capture is not None
        if matched and (not args.baseline_capture or not args.warmed_capture or not args.warmed_mode or active == target):
            raise ValueError("Matched evidence requires captures for both modes, --warmed-mode and a distinct --active-scene")
        extent = profile["width"], profile["height"]
        off_captures = read_captures(args.baseline_capture, digest, profile) if matched else None
        on_captures = read_captures(args.warmed_capture, digest, profile) if matched else None
        off, off_sessions, off_requests = collect_mode(
            args.baseline, target, active, False, model_mode, off_captures, extent)
        on, on_sessions, on_requests = collect_mode(
            args.warmed, target, active, True, model_mode, on_captures, extent)
        if off_sessions & on_sessions or off_requests & on_requests:
            raise ValueError("Baseline and warmed logs must use independent native runs")
        report = {
            "schema_version": 2 if matched else 1,
            "fixture_sha256": digest,
            "profile": profile,
            "scene_id": target,
            "active_scene_id": active,
            "active_frame_p95_ceiling_us": args.active_ceiling_us,
        }
        complete = True
        if matched:
            groups = compare_capture_groups(off, on, args.warmed_mode, args.active_ceiling_us)
            complete = all(group["status"] == "measured" for group in groups)
            passed = all(group["active_frame_passed"] is True for group in groups) if complete else None
            report["comparison_groups"] = groups
        else:
            if off["first_present_us"]["samples"] != on["first_present_us"]["samples"]:
                raise ValueError("Baseline and warmed runs need equal presented target load counts")
            passed = on["active_frame_us"]["p95"] <= args.active_ceiling_us
            report.update(baseline=off, warmed=on)
        report["active_frame_passed"] = passed
        if args.warmed_mode is not None:
            report["warmed_mode"] = args.warmed_mode
        with args.output.open("x", encoding="utf-8", newline="\n") as output:
            json.dump(report, output, indent=2)
            output.write("\n")
        print(f"Editor warming comparison written to {args.output}")
        return int(not passed) if complete else 2
    except (OSError, TypeError, ValueError, UnicodeError, RecursionError) as error:
        print(f"Cannot compare editor warming runs: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
