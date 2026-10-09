#!/usr/bin/env python3
"""Samples a whole process tree from /proc and checks it against targets.

Resource measurement for [#4](https://github.com/pane-app/pane/issues/4):
the workload driver (scripts/measure-linux.sh) runs Pane through a fixed
sequence and this script watches its process tree — Pane and every helper
it started — at a fixed cadence, so the record covers the full tree, not
one process. Each sample also holds every thread of the tree: its name,
its voluntary context switches (how often it blocked and was woken
again) and its CPU ticks, read from /proc/<pid>/task/<tid>, so the
summary can say which thread wakes how often (#189). Sampling is Linux
only: /proc is where it reads. The Windows script
(scripts/measure-windows.ps1) writes samples of the same shape itself, and
`summary` and `check` read either; what the macOS leg should measure is
recorded in docs/research/resource-measurements.md.

Usage:
  proc_tree.py watch <interval-seconds> <control-file> <samples-file>
      Samples until the control file's last line is "stop". Each earlier
      line is "<phase> <pid>": the phase the workload is in and the root
      of the tree to sample (0 while nothing runs). One JSON line per
      sample is appended to the samples file.
  proc_tree.py summary <samples> <events> <summary.json> [environment.json]
      Turns the samples and the workload's event lines into one summary
      record: per phase, RSS statistics, CPU seconds and share, growth,
      the wake-ups of the tree's threads by thread name (cut to 15 bytes,
      as Linux keeps it, on every platform), the window
      latencies the workload measured, and any extra values the events
      carried (how many extensions were installed, how many service
      cycles ran, ...).
  proc_tree.py check <summary.json> <targets.json>
      Compares the summary with the recorded targets. A null target is
      reported as pending, not checked; a phase the workload's record says
      it skipped is reported as skipped; a phase that was not measured
      otherwise, or a value above its ceiling, fails.
  proc_tree.py selfcheck
      Runs watch, summary and check against a throwaway process with
      children of its own, so the sampling logic is checked without a
      display or Pane: a sleep process stands in for the tree.
"""

import json
import os
import subprocess
import sys
import tempfile
import time

FORMAT = 1

if hasattr(os, "sysconf"):
    CLK_TCK = os.sysconf("SC_CLK_TCK") or 100
    PAGE_SIZE = os.sysconf("SC_PAGE_SIZE") or 4096
else:  # Not Linux: watch and selfcheck cannot work; summary and check can.
    CLK_TCK = 100
    PAGE_SIZE = 4096


def read_stat(pid, tid=None):
    """comm, parent pid and CPU ticks of one process, from /proc; of one
    of its threads with `tid` (the parent is then the process's)."""
    path = f"/proc/{pid}/stat" if tid is None else f"/proc/{pid}/task/{tid}/stat"
    with open(path, "rb") as handle:
        data = handle.read().decode("utf-8", "replace")
    close = data.rfind(")")
    comm = data[data.find("(") + 1 : close]
    fields = data[close + 2 :].split()
    # Field 4 (ppid) is fields[1], fields 14 and 15 (utime, stime) are
    # fields[11] and fields[12] once the comm is taken out.
    return comm, int(fields[1]), int(fields[11]) + int(fields[12])


def read_switches(pid, tid):
    """Voluntary and involuntary context switches of one thread, from
    /proc/<pid>/task/<tid>/status. A voluntary switch is the thread
    giving up the CPU to wait (a sleep, a lock, a read), so each one is a
    wake-up to come."""
    voluntary = involuntary = None
    with open(f"/proc/{pid}/task/{tid}/status", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            key, _, value = line.partition(":")
            if key == "voluntary_ctxt_switches":
                voluntary = int(value)
            elif key == "nonvoluntary_ctxt_switches":
                involuntary = int(value)
    if voluntary is None or involuntary is None:
        raise ValueError(f"/proc/{pid}/task/{tid}/status counts no context switches")
    return voluntary, involuntary


def read_threads(pid):
    """Every thread of one process: its id, its name (the kernel's comm,
    at most 15 bytes: a longer name set with pthread_setname_np is cut
    there), its context switches and its CPU ticks. A thread that ends
    between the reads is left out."""
    try:
        names = os.listdir(f"/proc/{pid}/task")
    except OSError:
        return []
    threads = []
    for name in names:
        if not name.isdigit():
            continue
        tid = int(name)
        try:
            comm, _, cpu_ticks = read_stat(pid, tid)
            voluntary, involuntary = read_switches(pid, tid)
        except (OSError, ValueError, IndexError):
            continue
        threads.append(
            {
                "tid": tid,
                "name": comm,
                "switches": voluntary,
                "involuntary": involuntary,
                "cpu_ticks": cpu_ticks,
            }
        )
    threads.sort(key=lambda thread: thread["tid"])
    return threads


def read_rss_kb(pid):
    """Resident set size in KiB, from /proc/<pid>/statm."""
    with open(f"/proc/{pid}/statm") as handle:
        resident_pages = int(handle.read().split()[1])
    return resident_pages * PAGE_SIZE // 1024


def children_of(pid):
    """The direct children of one process, or None where /proc does not
    offer them (a kernel without CONFIG_PROC_CHILDREN)."""
    try:
        with open(f"/proc/{pid}/task/{pid}/children") as handle:
            return [int(name) for name in handle.read().split()]
    except (OSError, ValueError):
        return None


def tree(root):
    """Every process at or below `root`, in pid order. The children files
    under /proc/task make this O(the tree); where they are missing, every
    process's parent is read instead (O(/proc))."""
    fallback = None

    def scanned_children(parent):
        # read_stat answers (comm, ppid, cpu ticks): the parent is the second.
        return [child for child, value in fallback.items() if value[1] == parent]

    found, stack = [], [root]
    while stack:
        pid = stack.pop()
        try:
            comm, ppid, cpu_ticks = read_stat(pid)
            rss_kb = read_rss_kb(pid)
        except (OSError, ValueError, IndexError):
            continue  # the process left between the two reads
        found.append(
            {
                "pid": pid,
                "ppid": ppid,
                "comm": comm,
                "rss_kb": rss_kb,
                "cpu_ticks": cpu_ticks,
                "threads": read_threads(pid),
            }
        )
        kids = children_of(pid)
        if kids is None:
            if fallback is None:
                fallback = {}
                try:
                    names = os.listdir("/proc")
                except OSError:
                    names = []
                for name in names:
                    if not name.isdigit():
                        continue
                    try:
                        fallback[int(name)] = read_stat(name)
                    except (OSError, ValueError, IndexError):
                        continue
            kids = scanned_children(pid)
        stack.extend(kids)
    found.sort(key=lambda process: process["pid"])
    return found


def sample(phase, root):
    """One sample of the tree rooted at `root` (empty once it is gone)."""
    processes = tree(root)
    return {
        "t": round(time.monotonic(), 3),
        "wall": round(time.time(), 3),
        "phase": phase,
        "root": root,
        # What a tick and a thread's "switches" are: the Windows script's
        # samples say 100 ns and every context switch instead.
        "tick_hz": CLK_TCK,
        "switch_kind": "voluntary",
        "processes": processes,
        "nproc": len(processes),
        "rss_kb": sum(process["rss_kb"] for process in processes),
        "cpu_ticks": sum(process["cpu_ticks"] for process in processes),
    }


def control_line(path):
    """The control file's last whole line, or None: the phase to sample."""
    try:
        with open(path, encoding="utf-8") as handle:
            text = handle.read()
    except OSError:
        return None
    lines = text.splitlines()
    if text and not text.endswith("\n"):
        lines = lines[:-1]  # a line without its newline may be half-written
    for line in reversed(lines):
        line = line.strip()
        if line:
            return line
    return None


def watch(interval, control, samples):
    """Sample until the control file says "stop" (see the module docstring)."""
    phase, root = "setup", 0
    with open(samples, "a", encoding="utf-8") as out:
        while True:
            line = control_line(control)
            if line == "stop":
                break
            if line:
                parts = line.split()
                if len(parts) == 2 and parts[1].isdigit():
                    phase, root = parts[0], int(parts[1])
                # A line that does not parse keeps the previous phase and root.
            out.write(json.dumps(sample(phase, root), separators=(",", ":")) + "\n")
            out.flush()
            time.sleep(interval)


def percentile(values, fraction):
    ordered = sorted(values)
    index = min(len(ordered) - 1, int(fraction * (len(ordered) - 1) + 0.5))
    return ordered[index]


def rss_stats(samples):
    """RSS statistics over the samples where the tree was alive."""
    values = [entry["rss_kb"] for entry in samples if entry["nproc"] > 0]
    if not values:
        return None
    return {
        "min": min(values),
        "median": percentile(values, 0.5),
        "p95": percentile(values, 0.95),
        "max": max(values),
    }


def cpu_seconds(samples):
    """The tree's CPU time across the phase, restarts included: the tick
    deltas between consecutive samples of one live root pid, summed. A
    process that leaves the tree between samples can lose its last ticks
    (a negative delta), so a delta is never subtracted."""
    total, previous = 0.0, None
    for entry in samples:
        if entry["nproc"] == 0:
            previous = None
            continue
        if previous and previous[0] == entry["root"]:
            total += max(0, entry["cpu_ticks"] - previous[1]) / tick_hz(entry)
        previous = (entry["root"], entry["cpu_ticks"])
    return round(total, 3)


def tick_hz(entry):
    """Ticks per second of a sample's CPU times: the sample says (the
    Windows script's count 100 ns), or this system's clock ticks (samples
    written before the field existed)."""
    return entry.get("tick_hz") or CLK_TCK


def thread_key(name):
    """A thread's name as summary.json lists it: cut to its first 15 bytes,
    as Linux keeps a thread's name (its comm), so that the Windows script's
    whole names and Linux's cut ones list the same thread under the same
    key ("pane-clipboard-expiry" is "pane-clipboard-" on both)."""
    return name.encode("utf-8")[:15].decode("utf-8", "ignore")


def wakeups(samples):
    """The wake-ups of the tree's threads across the phase, by thread name,
    or None when the samples hold no threads (a record from before #189).
    Names are cut to 15 bytes on every platform (`thread_key`).

    A thread's count is its switches at its last sample in the phase less
    those at its first, when it was there at the phase's first sample of
    its root; a thread first seen later was started within the phase, so
    all its switches count. A thread that ends between samples loses the
    switches since its last one, as the CPU deltas do. On Linux the count
    is voluntary context switches, each the thread waiting and being
    woken again; the Windows script counts every context switch of the
    thread, which for a thread that mostly sleeps is the same."""
    seen_roots, first, last, kinds = set(), {}, {}, set()
    threaded = False
    for entry in samples:
        if entry["nproc"] == 0:
            continue
        root = entry["root"]
        opening = root not in seen_roots
        seen_roots.add(root)
        hz = tick_hz(entry)
        kinds.add(entry.get("switch_kind", "voluntary"))
        for process in entry["processes"]:
            for thread in process.get("threads", ()):
                threaded = True
                key = (root, process["pid"], thread["tid"])
                now = (thread["switches"], thread["cpu_ticks"] / hz)
                if key not in first:
                    first[key] = now if opening else (0, 0.0)
                last[key] = (thread_key(thread["name"]),) + now
    if not threaded:
        return None
    by_name = {}
    for key, (name, switches, cpu) in last.items():
        start_switches, start_cpu = first[key]
        record = by_name.setdefault(name, {"threads": 0, "count": 0, "cpu_seconds": 0.0})
        record["threads"] += 1
        record["count"] += max(0, switches - start_switches)
        record["cpu_seconds"] += max(0.0, cpu - start_cpu)
    seconds = samples[-1]["t"] - samples[0]["t"]
    total = sum(record["count"] for record in by_name.values())
    for record in by_name.values():
        record["cpu_seconds"] = round(record["cpu_seconds"], 3)
        if seconds > 0:
            record["per_second"] = round(record["count"] / seconds, 2)
    summary = {
        "counted": "voluntary context switches" if kinds == {"voluntary"} else "context switches",
        "names": "cut to 15 bytes, as Linux keeps them",
        "total": total,
        "by_thread": by_name,
    }
    if seconds > 0:
        summary["per_second"] = round(total / seconds, 2)
    return summary


def growth_kb(samples):
    """The tree's RSS at the phase's last live sample minus its first."""
    values = [entry["rss_kb"] for entry in samples if entry["nproc"] > 0]
    if len(values) < 2:
        return None
    return values[-1] - values[0]


def summarize(samples, events):
    """One summary record from the workload's samples and events."""
    phases = {}
    for entry in samples:
        phases.setdefault(entry["phase"], []).append(entry)
    summary = {"format": FORMAT, "phases": {}}
    gaps = []
    for index, entry in enumerate(samples[1:], start=1):
        gaps.append(entry["t"] - samples[index - 1]["t"])
    if gaps:
        summary["sample_interval_seconds"] = percentile(gaps, 0.5)
    for phase, entries in phases.items():
        record = {
            "samples": len(entries),
            "max_processes": max(entry["nproc"] for entry in entries),
        }
        if len(entries) >= 2:
            record["seconds"] = round(entries[-1]["t"] - entries[0]["t"], 3)
        stats = rss_stats(entries)
        if stats:
            record["rss_kb"] = stats
        cpu = cpu_seconds(entries)
        if entries[-1]["t"] > entries[0]["t"]:
            record["cpu_seconds"] = cpu
            record["cpu_share"] = round(cpu / (entries[-1]["t"] - entries[0]["t"]), 4)
        growth = growth_kb(entries)
        if growth is not None:
            record["growth_kb"] = growth
        woken = wakeups(entries)
        if woken is not None:
            record["wakeups"] = woken
        summary["phases"][phase] = record
    for event in events:
        phase = event.get("phase")
        if phase not in phases:
            continue  # an event outside every sampled phase is a workload bug
        record = summary["phases"][phase]
        if event.get("event") == "window":
            record.setdefault("window_ms", {"runs": 0, "values": []})
            record["window_ms"]["runs"] += 1
            record["window_ms"]["values"].append(event["latency_ms"])
        for key, value in event.items():
            if key not in ("phase", "event", "latency_ms"):
                record[key] = value
    for record in summary["phases"].values():
        windows = record.pop("window_ms", None)
        if windows:
            record["window_ms"] = {
                "runs": windows["runs"],
                "median": percentile(windows["values"], 0.5),
                "max": max(windows["values"]),
            }
    installed = summary["phases"].get("installed-unused", {})
    idle = summary["phases"].get("idle-core", {})
    if installed.get("unused_packages") and installed.get("rss_kb") and idle.get("rss_kb"):
        extra = installed["rss_kb"]["median"] - idle["rss_kb"]["median"]
        installed["per_extension_kb"] = round(extra / installed["unused_packages"])
    return summary


def load_json_lines(path):
    records = []
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if line:
                records.append(json.loads(line))
    return records


def flatten(prefix, value):
    """(dotted path, leaf) pairs of a nested target description."""
    if isinstance(value, dict):
        for key, nested in value.items():
            yield from flatten(f"{prefix}.{key}" if prefix else key, nested)
    else:
        yield prefix, value


def lookup(record, path):
    for part in path.split("."):
        if not isinstance(record, dict) or part not in record:
            return None
        record = record[part]
    return record


def check(summary, targets):
    """Compare a summary with the targets; returns the failure lines. A
    phase the workload's record says it skipped (its environment's
    "skipped", as measure-linux.sh writes it when the hidden phase's
    payloads are absent) is reported as skipped, not as not measured."""
    problems, pending, passed, skipped = [], [], [], []
    skips = set(summary.get("environment", {}).get("skipped", ()))
    for phase, metrics in targets["targets"].items():
        record = summary.get("phases", {}).get(phase)
        if record is None:
            if phase in skips:
                skipped.append(f"{phase}: skipped by the workload")
            else:
                problems.append(f"{phase}: not measured")
            continue
        for path, ceiling in flatten("", metrics):
            name = f"{phase}.{path}"
            value = lookup(record, path)
            if ceiling is None:
                pending.append(f"{name}: pending a number"
                               + (f" (measured {value})" if value is not None else ""))
            elif value is None:
                problems.append(f"{name}: no value in the summary")
            elif value > ceiling:
                problems.append(f"{name}: {value} above the target {ceiling}")
            else:
                passed.append(f"{name}: {value} within {ceiling}")
    for line in passed:
        print(f"ok      {line}")
    for line in pending:
        print(f"pending {line}")
    for line in skipped:
        print(f"skipped {line}")
    for line in problems:
        print(f"FAIL    {line}")
    return problems


def wakeup_problems():
    """The wake-up arithmetic against hand-made samples, without /proc: a
    thread there from the phase's start counts what it did since, one
    started within the phase counts everything, a restart is a new root,
    and threads of one name are added up."""

    def thread(tid, name, switches, ticks):
        return {"tid": tid, "name": name, "switches": switches, "cpu_ticks": ticks}

    def entry(t, root, threads):
        processes = [{"pid": root, "threads": threads}] if root else []
        return {"t": t, "root": root, "nproc": len(processes), "tick_hz": 100,
                "processes": processes}

    samples = [
        entry(0.0, 10, [thread(10, "pane", 50, 100), thread(11, "tick", 1000, 10)]),
        entry(1.0, 10, [thread(10, "pane", 52, 101), thread(11, "tick", 1100, 11),
                        thread(12, "tick", 7, 0)]),
        entry(2.0, 0, []),
        entry(3.0, 20, [thread(20, "pane", 9, 40), thread(21, "tick", 300, 1)]),
        entry(4.0, 20, [thread(20, "pane", 10, 41), thread(21, "tick", 400, 2)]),
    ]
    woken = wakeups(samples)
    expected = {
        "counted": "voluntary context switches",
        "names": "cut to 15 bytes, as Linux keeps them",
        "total": 2 + 100 + 7 + 1 + 100,
        "per_second": round(210 / 4, 2),
        "by_thread": {
            "pane": {"threads": 2, "count": 3, "cpu_seconds": 0.02, "per_second": 0.75},
            "tick": {"threads": 3, "count": 207, "cpu_seconds": 0.02, "per_second": 51.75},
        },
    }
    if woken != expected:
        return [f"the wake-ups were not counted as expected: {woken} != {expected}"]
    if wakeups([entry(0.0, 10, []), entry(1.0, 0, [])]) is not None:
        return ["samples without threads were given wake-ups"]
    # A whole name, as the Windows script reads it, and Linux's cut one
    # are the same thread's.
    whole = [
        entry(0.0, 10, [thread(10, "pane-clipboard-expiry", 5, 0)]),
        entry(1.0, 10, [thread(10, "pane-clipboard-expiry", 6, 0)]),
    ]
    if list(wakeups(whole)["by_thread"]) != ["pane-clipboard-"]:
        return [f"a long thread name was not cut to 15 bytes: {wakeups(whole)}"]
    return []


def selfcheck():
    """watch, summary and check against a sleep process with children."""
    if not os.path.isdir("/proc"):
        print("selfcheck needs /proc; this is not Linux")
        return 1
    # The tree walk's fallback for a /proc without children files: the
    # same sleep process, found by scanning every process's parent.
    global children_of
    real_children_of = children_of
    scanned = subprocess.Popen(["sh", "-c", "sleep 6 & wait"])
    time.sleep(0.6)
    children_of = lambda pid: None
    try:
        scanned_tree = tree(scanned.pid)
    finally:
        children_of = real_children_of
    scanned.terminate()
    scanned.wait()
    if len(scanned_tree) != 2 or not any(p["comm"] == "sleep" for p in scanned_tree):
        print(f"FAIL    the parent-scan fallback did not find the tree: {scanned_tree}")
        return 1
    with tempfile.TemporaryDirectory() as work:
        control = os.path.join(work, "control")
        samples = os.path.join(work, "samples.jsonl")
        events = os.path.join(work, "events.jsonl")
        summary_path = os.path.join(work, "summary.json")
        targets_path = os.path.join(work, "targets.json")
        with open(control, "w", encoding="utf-8") as handle:
            handle.write("setup 0\n")
        process = subprocess.Popen(["sh", "-c", "sleep 8 & sleep 8 & wait"])
        watcher = subprocess.Popen(
            [sys.executable, os.path.abspath(__file__), "watch", "0.05", control, samples]
        )
        try:
            time.sleep(0.4)
            with open(control, "a", encoding="utf-8") as handle:
                handle.write(f"one {process.pid}\n")
            time.sleep(0.5)
            with open(control, "a", encoding="utf-8") as handle:
                handle.write(f"two {process.pid}\n")
            time.sleep(0.5)
            with open(events, "w", encoding="utf-8") as handle:
                handle.write(json.dumps({"phase": "two", "event": "window", "latency_ms": 12}) + "\n")
                handle.write(json.dumps({"phase": "two", "extensions": 7}) + "\n")
                handle.write(json.dumps({"phase": "nowhere", "event": "window", "latency_ms": 1}) + "\n")
        finally:
            with open(control, "a", encoding="utf-8") as handle:
                handle.write("stop\n")
            watcher.wait(timeout=30)
            process.terminate()
            process.wait()
        summary = summarize(load_json_lines(samples), load_json_lines(events))
        problems = []
        for phase in ("one", "two"):
            record = summary["phases"].get(phase)
            if not record or record["samples"] < 4:
                problems.append(f"{phase}: too few samples: {record}")
                continue
            if record["max_processes"] != 3:
                problems.append(f"{phase}: the tree should hold 3 processes: {record}")
            if record["rss_kb"]["median"] <= 0:
                problems.append(f"{phase}: no RSS measured: {record}")
        two = summary["phases"]["two"]
        if two["window_ms"] != {"runs": 1, "median": 12, "max": 12}:
            problems.append(f"two: the window event did not land: {two}")
        if two["extensions"] != 7:
            problems.append(f"two: the event's extra value did not land: {two}")
        # Every thread of the tree is counted by its name: the shell's and
        # the two sleeps' (one thread each).
        threads = two.get("wakeups", {}).get("by_thread", {})
        if threads.get("sleep", {}).get("threads") != 2 or "sh" not in threads:
            problems.append(f"two: the threads were not counted by name: {two.get('wakeups')}")
        problems.extend(wakeup_problems())
        with open(summary_path, "w", encoding="utf-8") as handle:
            json.dump(summary, handle, indent=2, sort_keys=True)
        with open(targets_path, "w", encoding="utf-8") as handle:
            json.dump(
                {
                    "targets": {
                        "one": {"rss_kb": {"max": 2_000_000}, "cpu_share": None},
                        "two": {"rss_kb": {"max": 1}},
                        "gone": {"growth_kb": 1},
                        "left": {"growth_kb": 1},
                    }
                },
                handle,
            )
        # The workload skipped "left": reported so, and not failed.
        summary["environment"] = {"skipped": ["left"]}
        import io
        import contextlib

        printed = io.StringIO()
        with contextlib.redirect_stdout(printed):
            failures = check(summary, json.load(open(targets_path, encoding="utf-8")))
        if "two.rss_kb.max" not in " ".join(failures):
            problems.append(f"a breach was not reported: {failures}")
        if "gone: not measured" not in failures:
            problems.append(f"an unmeasured phase was not reported: {failures}")
        if "one.cpu_share: pending" not in printed.getvalue():
            problems.append(f"a null target was not pending: {printed.getvalue()!r}")
        if any(failure.startswith("left") for failure in failures) \
                or "skipped left" not in printed.getvalue():
            problems.append(f"a skipped phase was not reported as skipped: {failures}")
        if problems:
            for problem in problems:
                print(f"FAIL    {problem}")
            return 1
        counted = sum(record["samples"] for record in summary["phases"].values())
        print(f"selfcheck passed: {counted} samples, 3 processes, summary and check behave")
        return 0


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    if argv[1] == "selfcheck" and len(argv) == 2:
        return selfcheck()
    if len(argv) == 5 and argv[1] == "watch":
        watch(float(argv[2]), argv[3], argv[4])
        return 0
    if argv[1] == "summary" and len(argv) in (5, 6):
        summary = summarize(load_json_lines(argv[2]), load_json_lines(argv[3]))
        if len(argv) == 6:
            with open(argv[5], encoding="utf-8") as handle:
                summary["environment"] = json.load(handle)
        with open(argv[4], "w", encoding="utf-8") as handle:
            json.dump(summary, handle, indent=2, sort_keys=True)
        return 0
    if len(argv) == 4 and argv[1] == "check":
        with open(argv[2], encoding="utf-8") as handle:
            summary = json.load(handle)
        with open(argv[3], encoding="utf-8") as handle:
            targets = json.load(handle)
        return 1 if check(summary, targets) else 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
