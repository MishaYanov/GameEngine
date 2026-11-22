"""Plan and review local, matched native editor warming captures."""

import argparse
from collections import defaultdict
from hashlib import sha256
from itertools import product
from pathlib import Path
import re
import stat
from statistics import stdev
import sys

from compare_playtrace import read_local_json
from measure_editor_warming import (
    MODEL_MODES, capture_metrics, collect_mode, compare_capture_groups, read_captures, scene_id,
)
from measure_scene_load import (
    MAX_LOGS, MAX_METADATA_BYTES, MAX_MICROSECONDS, MAX_SAMPLES, fixture_id, integer, validate_profile,
)
from rehearse_release import ordinary_mode
from review_candidate import _json_bytes, _write_new


CACHE_STATES = ("fresh", "retained", "invalidated")
PLAN_FIELDS = {
    "schema_version", "name", "fixture_sha256", "profile_sha256", "scene_pairs", "modes",
    "cache_states", "repeats", "presentations_per_repeat",
}
SLOT_FIELDS = ("source", "target", "mode", "cache_state", "repeat")
RUN_FIELDS = {"schema_version", "plan_sha256", "fixture_sha256", "profile_sha256", "slot"}
BUDGET_FIELDS = {
    "schema_version", "plan_name", "plan_sha256",
    "max_first_present_p95_increase_us", "max_active_frame_p95_us",
}
MAX_OUTPUT_BYTES = 8 * 1024 * 1024


def read_schema(path, fields, label):
    record, raw = read_local_json(path, MAX_METADATA_BYTES)
    if (not isinstance(record, dict) or record.keys() != fields
            or not integer(record["schema_version"], 1, 1)):
        raise ValueError(f"Expected {label} schema 1")
    return record, sha256(raw).hexdigest()


def choices(values, allowed, label):
    if (not isinstance(values, list) or not values
            or any(not isinstance(value, str) or value not in allowed for value in values)
            or len(set(values)) != len(values)):
        raise ValueError(f"Invalid or repeated {label}")
    return values


def read_plan(path, profile_path):
    plan, digest = read_schema(path, PLAN_FIELDS, "warming plan")
    if (not isinstance(plan["name"], str)
            or re.fullmatch(r"[a-z0-9][a-z0-9_-]{0,63}", plan["name"]) is None):
        raise ValueError("Plan name needs a lowercase identifier")
    fixture_id(plan["fixture_sha256"])
    profile, raw = read_local_json(profile_path, MAX_METADATA_BYTES)
    validate_profile(profile)
    if fixture_id(plan["profile_sha256"]) != sha256(raw).hexdigest():
        raise ValueError("Viewport profile bytes do not match the plan")
    pairs = plan["scene_pairs"]
    if not isinstance(pairs, list) or not 1 <= len(pairs) <= 8:
        raise ValueError("Plan needs one to eight directional scene pairs")
    directions = set()
    for pair in pairs:
        if not isinstance(pair, dict) or pair.keys() != {"source", "target"}:
            raise ValueError("Each scene pair needs a source and target")
        source, target = scene_id(pair["source"]), scene_id(pair["target"])
        if source == target or (source, target) in directions:
            raise ValueError("Scene pairs must use distinct scenes and unique directions")
        directions.add((source, target))
    modes = choices(plan["modes"], MODEL_MODES, "warming modes")
    if "off" not in modes or len(modes) < 2:
        raise ValueError("Plan needs Off and at least one warmed mode")
    states = choices(plan["cache_states"], CACHE_STATES, "cache states")
    if (not integer(plan["repeats"], 10, 1)
            or not integer(plan["presentations_per_repeat"], 15, 1)):
        raise ValueError("Plan allows 1–10 repeats and 1–15 presentations per repeat")
    slots = [
        (source, target, mode, state, repeat)
        for (source, target), mode, state, repeat in product(
            sorted(directions), sorted(modes), sorted(states), range(1, plan["repeats"] + 1)
        )
    ]
    if len(slots) > MAX_LOGS:
        raise ValueError(f"Plan supports at most {MAX_LOGS} runs")
    return plan, digest, profile, slots


def slot_row(slot):
    return dict(zip(SLOT_FIELDS, slot))


def run_slot(record, plan, digest):
    if any(record[field] != wanted for field, wanted in (
        ("fixture_sha256", plan["fixture_sha256"]),
        ("profile_sha256", plan["profile_sha256"]),
        ("plan_sha256", digest),
    )):
        raise ValueError("Run metadata does not match the plan, fixture and exact profile")
    slot = record["slot"]
    if (not isinstance(slot, dict) or slot.keys() != set(SLOT_FIELDS)
            or not integer(slot["repeat"], 10, 1)
            or not isinstance(slot["mode"], str) or slot["mode"] not in MODEL_MODES
            or slot["cache_state"] not in CACHE_STATES):
        raise ValueError("Invalid run slot")
    if scene_id(slot["source"]) == scene_id(slot["target"]):
        raise ValueError("Run slot must specify a directional scene pair")
    return tuple(slot[field] for field in SLOT_FIELDS)


def review_runs(inputs, expected, plan, digest, profile, profile_path):
    if len(inputs) > MAX_LOGS:
        raise ValueError(f"Supply at most {MAX_LOGS} warming runs")
    expected = set(expected)
    runs, unexpected = {}, []
    sessions, requests, submitted = set(), set(), set()
    for metadata, report, log in inputs:
        record, _ = read_schema(metadata, RUN_FIELDS, "warming run")
        slot = run_slot(record, plan, digest)
        if slot in submitted:
            raise ValueError("Duplicate warming run slot")
        submitted.add(slot)
        if slot not in expected:
            unexpected.append(slot)
            continue
        source, target, mode, state, _ = slot
        captures = read_captures([(report, plan["fixture_sha256"], profile_path)],
                                 plan["fixture_sha256"], profile)
        groups, run_sessions, run_requests = collect_mode(
            [log], target, source, mode != "off", mode, captures, (profile["width"], profile["height"]),
        )
        if run_sessions != captures.keys() or sessions & run_sessions or requests & run_requests:
            raise ValueError("Every run needs a separate native session and request identifiers")
        if any(dict(key[0])["state"] != state for key in groups):
            raise ValueError("Observed native cache state does not match the run slot")
        sessions.update(run_sessions)
        requests.update(run_requests)
        runs[slot] = (next(iter(run_sessions)), groups)
    return runs, sorted(unexpected)


def mode_metrics(group, mode):
    metrics = capture_metrics(group)
    metrics["model_warming"]["zero_adoption_samples"] = zero_adoptions(group)
    return {"mode": mode, **metrics}


def zero_adoptions(group):
    return sum(row["consumed_models"] == 0 for row in group["models"])


def coverage_report(slots, runs, unexpected, required):
    captured = []
    incomplete = set()
    for slot in slots:
        if slot not in runs:
            incomplete.add(slot)
            continue
        session, groups = runs[slot]
        qualifying = sum(len(group["presented"]) for group in groups.values())
        if qualifying != required:
            incomplete.add(slot)
        captured.append({
            **slot_row(slot),
            "session_id": session,
            "status": "complete" if qualifying == required else "short" if qualifying < required else "excess",
            "qualifying_presentations": qualifying,
            "observed": [
                {"pre_switch_cache": dict(cache), "viewport": dict(viewport),
                 **mode_metrics(groups[cache, viewport], slot[2])}
                for cache, viewport in sorted(groups)
            ],
        })
    return {
        "complete": not incomplete and not unexpected,
        "expected_runs": len(slots),
        "presentations_per_repeat": required,
        "missing": [slot_row(slot) for slot in slots if slot not in runs],
        "unexpected": [slot_row(slot) for slot in unexpected],
        "runs": captured,
    }, incomplete


def paired_result(key, off, warmed, mode, budget, matched_repeats):
    if not matched_repeats or off is None or warmed is None or len(off["presented"]) > MAX_SAMPLES:
        return {
            "pre_switch_cache": dict(key[0]), "viewport": dict(key[1]),
            "status": "insufficient_measurements", "reason": "unmatched_presentations",
            "baseline": mode_metrics(off, "off") if off else None,
            "warmed": mode_metrics(warmed, mode) if warmed else None,
        }
    ceiling = budget["max_active_frame_p95_us"] if budget else MAX_MICROSECONDS
    result, = compare_capture_groups({key: off}, {key: warmed}, mode, ceiling)
    for label, group in (("baseline", off), ("warmed", warmed)):
        result[label]["model_warming"]["zero_adoption_samples"] = zero_adoptions(group)
    if budget is None:
        # Coverage reports have no timing ceiling.
        del result["active_frame_passed"]
        return result
    if result["status"] != "measured":
        result["status"] = "insufficient_measurements"
        result["reason"] = "sample_minimum"
        return result
    for label, group in (("baseline", off), ("warmed", warmed)):
        for metric, samples in (("first_present_us", "presented"), ("active_frame_us", "frames")):
            result[label][metric]["sample_stddev"] = round(stdev(group[samples]), 2)
    increase = result["warmed"]["first_present_us"]["p95"] - result["baseline"]["first_present_us"]["p95"]
    result["first_present_p95_increase_us"] = increase
    passes = result["active_frame_passed"] and increase <= budget["max_first_present_p95_increase_us"]
    result["status"] = "within_budget" if passes else "over_budget"
    return result


def comparisons(slots, runs, incomplete, budget):
    pooled = defaultdict(lambda: defaultdict(lambda: {
        "loads": [], "presented": [], "models": [], "frames": [],
    }))
    for (source, target, mode, state, _), (_, groups) in runs.items():
        for key, group in groups.items():
            aggregate = pooled[source, target, mode, state][key]
            for metric, values in group.items():
                aggregate[metric].extend(values)
    unavailable = {slot[:4] for slot in incomplete}
    repeats = sorted({slot[4] for slot in slots})
    results = []
    for source, target, mode, state in sorted({slot[:4] for slot in slots if slot[2] != "off"}):
        active = source, target, mode, state
        baseline = source, target, "off", state
        result = {"source": source, "target": target, "mode": mode, "cache_state": state}
        if active in unavailable or baseline in unavailable:
            results.append(result | {"status": "insufficient_measurements", "reason": "incomplete_coverage"})
            continue
        off, warmed = pooled[baseline], pooled[active]
        for key in sorted(off.keys() | warmed.keys()):
            matched = all(
                presentation_count(runs[(*baseline, repeat)][1], key)
                == presentation_count(runs[(*active, repeat)][1], key)
                for repeat in repeats
            )
            results.append(result | paired_result(key, off.get(key), warmed.get(key), mode, budget, matched))
    return results


def presentation_count(groups, key):
    group = groups.get(key)
    return len(group["presented"]) if group is not None else 0


def read_budget(path, plan, digest):
    budget, budget_digest = read_schema(path, BUDGET_FIELDS, "warming budget")
    if (budget["plan_name"] != plan["name"] or budget["plan_sha256"] != digest):
        raise ValueError("Budget does not match the named plan and its exact bytes")
    if not all(integer(budget[field], MAX_MICROSECONDS) for field in (
        "max_first_present_p95_increase_us", "max_active_frame_p95_us",
    )):
        raise ValueError("Budget thresholds must be nonnegative integer microseconds")
    return budget, budget_digest


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("plan", "plan-coverage", "budget-review"):
        command = commands.add_parser(name)
        command.add_argument("--plan", type=Path, required=True)
        command.add_argument("--profile", type=Path, required=True)
        command.add_argument("--output", type=Path, required=True)
        if name != "plan":
            command.add_argument("--run", type=Path, nargs=3, action="append", default=[],
                                 metavar=("METADATA", "NATIVE_REPORT", "LOG_JSONL"))
        if name == "budget-review":
            command.add_argument("--budget", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        if not stat.S_ISDIR(ordinary_mode(args.output.parent)):
            raise ValueError("Warming review output parent must be an ordinary directory")
        if args.output.exists() or args.output.is_symlink():
            raise FileExistsError("Warming review output already exists")
        plan, digest, profile, slots = read_plan(args.plan, args.profile)
        report = dict(plan, plan_sha256=digest, profile=profile)
        if args.command == "plan":
            report["slots"] = [slot_row(slot) for slot in slots]
        else:
            budget = None
            if args.command == "budget-review":
                budget, budget_digest = read_budget(args.budget, plan, digest)
                report.update(budget=budget, budget_sha256=budget_digest)
            runs, unexpected = review_runs(args.run, slots, plan, digest, profile, args.profile)
            coverage, incomplete = coverage_report(slots, runs, unexpected, plan["presentations_per_repeat"])
            results = comparisons(slots, runs, incomplete, budget)
            report.update(coverage=coverage, comparisons=results)
        _write_new(args.output, _json_bytes(report), MAX_OUTPUT_BYTES)
    except (OSError, TypeError, ValueError, UnicodeError, RecursionError) as error:
        print(f"Cannot review warming experiment: {error}", file=sys.stderr)
        return 3
    if args.command == "plan":
        return 0
    if not coverage["complete"]:
        return 2
    if any(result["status"] == "over_budget" for result in results):
        return 1
    return 0 if all(result["status"] in {"measured", "within_budget"} for result in results) else 2


if __name__ == "__main__":
    raise SystemExit(main())
