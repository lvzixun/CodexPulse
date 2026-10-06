#!/usr/bin/env python3
"""Summarize read-only macOS samples, separating native window lifecycle states.

Physical footprint is not Windows private working set. CPU is normalized to the
sampled machine's total logical CPU capacity. Missing and transitioning intervals
are excluded and their coverage is reported; no acceptance pass is inferred.
"""

import argparse
import bisect
import datetime
import json
import math
from pathlib import Path


def timestamp(value):
    result = datetime.datetime.fromisoformat(value)
    if result.tzinfo is None:
        raise ValueError("timestamps must include a timezone")
    return result.timestamp()


def records(path):
    with Path(path).open() as stream:
        for line in stream:
            if line.strip():
                yield json.loads(line)


def summarize(path, lifecycle):
    rows = list(records(path))
    if not rows or rows[0].get("kind") != "metadata":
        raise ValueError("sampling metadata is missing")
    metadata = rows[0]
    samples = rows[1:]
    if not samples or any(row.get("kind") != "sample" for row in samples):
        raise ValueError("sample records are missing or invalid")
    elapsed = [row["elapsed_s"] for row in samples]
    if any(b <= a for a, b in zip(elapsed, elapsed[1:])):
        raise ValueError("sample monotonic times must strictly increase")
    utc = [timestamp(row["utc"]) for row in samples]
    if any(b <= a for a, b in zip(utc, utc[1:])):
        raise ValueError("wall clock changed; lifecycle alignment cannot be trusted")
    host = metadata.get("host")
    events = []
    if lifecycle:
        if not host:
            raise ValueError("lifecycle alignment requires a --host sampling run")
        paths = lifecycle if isinstance(lifecycle, list) else [lifecycle]
        history = {}
        for source in paths:
            for row in records(source):
                if row.get("pid") != host:
                    continue
                sequence = row["sequence"]
                if sequence in history and history[sequence] != row:
                    raise ValueError("conflicting lifecycle records for the same host sequence")
                history[sequence] = row
        sequences = sorted(history)
        if any(b != a + 1 for a, b in zip(sequences, sequences[1:])):
            raise ValueError("lifecycle history has gaps; state alignment cannot be trusted")
        for _, row in sorted(history.items()):
            if row.get("pid") == host and row.get("event") in {
                "created", "shown", "hidden", "destroyed"
            }:
                events.append((timestamp(row["utc"]), row["event"], row["window_generation"]))
        if any(b[0] < a[0] for a, b in zip(events, events[1:])):
            raise ValueError("lifecycle timestamps are not ordered")
    event_times = [event[0] for event in events]
    groups = {}
    previous_event = None
    run_started = elapsed[0]
    for index, row in enumerate(samples):
        event_index = bisect.bisect_right(event_times, utc[index]) - 1
        state, generation = "unclassified", None
        if event_index >= 0:
            _, event, generation = events[event_index]
            state = {
                "created": "hidden_loaded", "shown": "visible",
                "hidden": "hidden_loaded", "destroyed": "hidden_released",
            }[event]
        group = groups.setdefault(state, {
            "samples": 0, "complete_samples": 0, "footprints": [],
            "member_counts": set(), "cpu_capacity_percent_seconds": 0,
            "cpu_covered_seconds": 0, "observed_interval_seconds": 0,
            "longest_observed_run_seconds": 0,
        })
        if (event_index, generation) != previous_event:
            run_started = elapsed[index]
        group["longest_observed_run_seconds"] = max(
            group["longest_observed_run_seconds"], elapsed[index] - run_started
        )
        group["samples"] += 1
        if row["specified_group_complete"]:
            group["complete_samples"] += 1
            group["footprints"].append(row["group_physical_footprint_bytes"] / 2**20)
            group["member_counts"].add(len(row["processes"]))
        # A CPU delta belongs to this state only when no lifecycle transition
        # occurred between its two endpoints, even if the state names match.
        if index and (event_index, generation) == previous_event:
            duration = elapsed[index] - elapsed[index - 1]
            group["observed_interval_seconds"] += duration
            cpu = row.get("group_cpu_capacity_percent")
            if cpu is not None and math.isfinite(cpu):
                group["cpu_capacity_percent_seconds"] += cpu * duration
                group["cpu_covered_seconds"] += duration
        previous_event = (event_index, generation)
    for group in groups.values():
        values = sorted(group.pop("footprints"))
        group["physical_footprint_mib_p95"] = (
            values[math.ceil(len(values) * 0.95) - 1] if values else None
        )
        group["physical_footprint_mib_min"] = min(values) if values else None
        group["physical_footprint_mib_max"] = max(values) if values else None
        group["member_counts"] = sorted(group["member_counts"])
        weighted = group.pop("cpu_capacity_percent_seconds")
        covered = group["cpu_covered_seconds"]
        group["cpu_capacity_percent_mean"] = weighted / covered if covered else None
    span = elapsed[-1] - elapsed[0]
    return {
        "metadata": metadata,
        "first_sample_utc": samples[0]["utc"],
        "last_sample_utc": samples[-1]["utc"],
        "observed_seconds": span,
        "requested_duration_reached": elapsed[-1] >= metadata["duration_s"],
        "eight_hours_observed": span >= 28800,
        "ownership_verified_all_samples": all(row.get("ownership_verified") for row in samples),
        "states": groups,
        "acceptance": "not_inferred",
        "drift": "not_inferred: requires matched, warmed, uninterrupted endpoint states",
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("samples")
    parser.add_argument("--lifecycle", action="append",
                        help="lifecycle file; repeat for rotated logs or an earlier capture")
    args = parser.parse_args()
    print(json.dumps(summarize(args.samples, args.lifecycle), indent=2))


if __name__ == "__main__":
    main()
