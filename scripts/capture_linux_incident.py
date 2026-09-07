#!/usr/bin/env python3
"""Capture a live Linux process without relying on its event loop (#9942)."""

import argparse
import datetime
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time


MAX_FILE_BYTES = 4 * 1024 * 1024


def parse_stat(text):
    # comm may contain spaces and closing parentheses; fields start after the
    # final ')'. Index zero here is Linux stat field 3 (state).
    if not isinstance(text, str):
        raise ValueError("stat is unavailable")
    end = text.rfind(")")
    start = text.find("(")
    if start < 0 or end <= start:
        raise ValueError("malformed /proc stat comm")
    fields = text[end + 1:].split()
    return {
        "comm": text[start + 1:end],
        "state": fields[0],
        "cpu_ticks": int(fields[11]) + int(fields[12]),
        "start_ticks": int(fields[19]),
        "virtual_bytes": int(fields[20]),
        "rss_pages": int(fields[21]),
    }


def read_file(path, errors):
    try:
        with path.open("rb") as source:
            data = source.read(MAX_FILE_BYTES + 1)
        if len(data) > MAX_FILE_BYTES:
            errors.append(f"{path}: truncated at {MAX_FILE_BYTES} bytes")
        return data[:MAX_FILE_BYTES].decode("utf-8", errors="replace")
    except OSError as error:
        errors.append(f"{path}: {error}")
        return None


def identity(process):
    return parse_stat((process / "stat").read_text())["start_ticks"]


def snapshot(process, expected_start, max_threads=256):
    if identity(process) != expected_start:
        raise RuntimeError("PID was reused; refusing to mix two processes")
    result = {"monotonic_seconds": time.monotonic(), "files": {}, "threads": {}, "errors": []}
    for name in ("stat", "status", "smaps_rollup", "io", "limits", "maps"):
        result["files"][name] = read_file(process / name, result["errors"])
    tasks = sorted((process / "task").iterdir(), key=lambda path: int(path.name))
    result["threads_seen"] = len(tasks)
    if len(tasks) > max_threads:
        result["errors"].append(f"thread capture limited to {max_threads} of {len(tasks)} threads")
    for task in tasks[:max_threads]:
        result["threads"][task.name] = {
            name: read_file(task / name, result["errors"])
            for name in ("stat", "wchan", "stack", "schedstat")
        }
    if identity(process) != expected_start:
        raise RuntimeError("PID changed during capture; discarding this sample")
    return result


def summarize(samples, ticks_per_second, page_size):
    if len(samples) < 2:
        return {"intervals": [], "note": "Need two samples to measure growth and CPU."}
    intervals = []
    for before, after in zip(samples, samples[1:]):
        elapsed = after["monotonic_seconds"] - before["monotonic_seconds"]
        if elapsed <= 0:
            continue
        row = {"elapsed_seconds": elapsed, "threads": [], "errors": []}
        try:
            first = parse_stat(before["files"]["stat"])
            last = parse_stat(after["files"]["stat"])
            growth = (last["rss_pages"] - first["rss_pages"]) * page_size
            row.update(rss_bytes=last["rss_pages"] * page_size,
                       rss_delta_bytes=growth, rss_bytes_per_second=growth / elapsed)
        except (TypeError, ValueError, IndexError) as error:
            row["errors"].append(f"process stat unavailable: {error}")
        for tid, thread in after["threads"].items():
            previous = before["threads"].get(tid)
            if previous is None:
                continue
            try:
                first, last = parse_stat(previous["stat"]), parse_stat(thread["stat"])
                if first["start_ticks"] != last["start_ticks"]:
                    continue  # A recycled thread ID is not a CPU delta.
                cpu_seconds = (last["cpu_ticks"] - first["cpu_ticks"]) / ticks_per_second
                if cpu_seconds < 0:
                    continue
                row["threads"].append({"tid": int(tid), "comm": last["comm"],
                    "state": last["state"], "cpu_seconds": cpu_seconds,
                    "cpu_percent_one_core": 100 * cpu_seconds / elapsed,
                    "wchan": thread["wchan"]})
            except (TypeError, ValueError, IndexError) as error:
                row["errors"].append(f"thread {tid} stat unavailable: {error}")
        row["threads"].sort(key=lambda item: item["cpu_seconds"], reverse=True)
        intervals.append(row)
    return {"intervals": intervals}


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pid", type=int)
    parser.add_argument("--output", required=True, type=Path, help="new directory; never overwrites a capture")
    parser.add_argument("--samples", type=int, default=6)
    parser.add_argument("--interval", type=float, default=2)
    parser.add_argument("--max-threads", type=int, default=256)
    parser.add_argument("--perf-seconds", type=int, default=0,
                        help="optionally record user-space call chains with perf for 1–60 seconds")
    args = parser.parse_args(argv)
    if platform.system() != "Linux":
        parser.error("this collector requires Linux /proc")
    if args.pid <= 0 or not 2 <= args.samples <= 3600 or not 0.1 <= args.interval <= 60:
        parser.error("require positive PID, 2–3600 samples and interval 0.1–60 seconds")
    if not 1 <= args.max_threads <= 4096 or not 0 <= args.perf_seconds <= 60:
        parser.error("require max-threads 1–4096 and perf-seconds 0–60")
    process = Path("/proc") / str(args.pid)
    try:
        expected_start = identity(process)
        args.output.mkdir(mode=0o700, parents=False, exist_ok=False)
    except (OSError, ValueError, IndexError) as error:
        parser.error(str(error))
    metadata = {"pid": args.pid, "start_ticks": expected_start,
                "captured_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                "kernel": platform.release(), "machine": platform.machine(),
                "ticks_per_second": os.sysconf("SC_CLK_TCK"),
                "page_size": os.sysconf("SC_PAGE_SIZE"), "errors": []}
    try:
        metadata["executable"] = os.readlink(process / "exe")
    except OSError as error:
        metadata["errors"].append(str(error))
    samples = []
    perf = None
    perf_log = None
    try:
        if args.perf_seconds:
            command = ["perf", "record", "-F", "99", "-g", "-p", str(args.pid),
                       "-o", str(args.output / "perf.data"), "--", "sleep", str(args.perf_seconds)]
            metadata["perf_command"] = command
            perf_log = (args.output / "perf.log").open("w")
            try:
                perf = subprocess.Popen(command, stdout=perf_log, stderr=perf_log)
            except OSError as error:
                metadata["errors"].append(f"perf unavailable: {error}")
        for index in range(args.samples):
            try:
                sample = snapshot(process, expected_start, args.max_threads)
            except (OSError, RuntimeError, ValueError, IndexError) as error:
                metadata["errors"].append(str(error))
                break
            samples.append(sample)
            write_json(args.output / f"sample-{index:04d}.json", sample)
            if index + 1 < args.samples:
                time.sleep(args.interval)
        if perf is not None:
            try:
                metadata["perf_exit_code"] = perf.wait(timeout=args.perf_seconds + 5)
            except subprocess.TimeoutExpired:
                metadata["errors"].append("perf exceeded its deadline")
    except KeyboardInterrupt:
        metadata["errors"].append("capture interrupted; partial samples preserved")
    finally:
        if perf is not None and perf.poll() is None:
            # Stop only the collector we launched, never the observed process.
            perf.terminate()
            try:
                perf.wait(timeout=5)
            except subprocess.TimeoutExpired:
                perf.kill()
                perf.wait()
        if perf_log is not None:
            perf_log.close()
        metadata["samples_captured"] = len(samples)
        metadata["samples_requested"] = args.samples
        write_json(args.output / "metadata.json", metadata)
        write_json(args.output / "summary.json", summarize(
            samples, metadata["ticks_per_second"], metadata["page_size"]))
    print(f"Captured {len(samples)}/{args.samples} samples in {args.output}")
    if args.perf_seconds:
        print("User-space stacks: inspect perf.log, then perf report --stdio -i <output>/perf.data")
    return 0 if len(samples) == args.samples and not metadata["errors"] and metadata.get("perf_exit_code", 0) == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
