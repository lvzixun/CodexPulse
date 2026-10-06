#!/usr/bin/env python3
"""Read-only macOS sampling, with optional dynamic responsible-process ownership.

WebKit XPC processes often have launchd as their parent. Supply IDs whose
ownership was verified separately, or use --host for kernel-backed responsibility
lookup. That development-only SPI is also used by Chromium's process_info_mac.mm;
it is not linked into CodexPulse. This tool never guesses from a process name.
Physical footprint is a macOS metric, not Windows private working set.
"""

import argparse
import ctypes
import datetime
import json
import os
import sys
import time


class Usage(ctypes.Structure):
    # SDK sys/resource.h: struct rusage_info_v2, available since macOS 10.9.
    _fields_ = [("uuid", ctypes.c_ubyte * 16)] + [
        (name, ctypes.c_uint64)
        for name in (
            "user system idle interrupt pageins wired resident footprint start exit "
            "child_user child_system child_idle child_interrupt child_pageins "
            "child_elapsed disk_read disk_write"
        ).split()
    ]


class Timebase(ctypes.Structure):
    _fields_ = [("numer", ctypes.c_uint32), ("denom", ctypes.c_uint32)]


def positive(value):
    number = float(value)
    if not 0 < number < float("inf"):
        raise argparse.ArgumentTypeError("must be a finite positive number")
    return number


def responsible_pids(proc, lookup, host):
    if lookup(host) != host:
        raise RuntimeError("host is unavailable or is not self-responsible")
    capacity = max(128, proc.proc_listallpids(None, 0) + 128)
    for _ in range(3):
        if capacity > 65536:
            break
        buffer = (ctypes.c_int * capacity)()
        count = proc.proc_listallpids(buffer, ctypes.sizeof(buffer))
        if count <= 0:
            break
        if count < capacity:
            members = sorted({pid for pid in buffer[:count]
                              if pid > 0 and lookup(pid) == host})
            if host not in members:
                break
            return members
        capacity *= 2
    raise RuntimeError("cannot enumerate a verified responsible-process group")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pids", nargs="*", type=int)
    parser.add_argument("--host", type=int,
                        help="discover all processes responsible to this live application PID")
    parser.add_argument("--duration", type=positive, default=600)
    parser.add_argument("--interval", type=positive, default=5)
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("requires macOS")
    if bool(args.pids) == (args.host is not None):
        parser.error("provide either explicit process IDs or --host")
    if args.host is not None and args.host <= 0:
        parser.error("host must be a positive process ID")
    if any(pid <= 0 for pid in args.pids) or len(set(args.pids)) != len(args.pids):
        parser.error("provide distinct positive process IDs")

    proc = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
    proc.proc_pid_rusage.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_void_p]
    proc.proc_pid_rusage.restype = ctypes.c_int
    proc.proc_listallpids.argtypes = [ctypes.c_void_p, ctypes.c_int]
    proc.proc_listallpids.restype = ctypes.c_int
    system = ctypes.CDLL("/usr/lib/libSystem.B.dylib")
    lookup = None
    if args.host is not None:
        try:
            lookup = system.responsibility_get_pid_responsible_for_pid
        except AttributeError as error:
            raise RuntimeError("responsible-process SPI is unavailable on this macOS") from error
        lookup.argtypes = [ctypes.c_int]
        lookup.restype = ctypes.c_int
    system.mach_timebase_info.argtypes = [ctypes.POINTER(Timebase)]
    base = Timebase()
    if system.mach_timebase_info(ctypes.byref(base)) != 0 or not base.denom:
        raise RuntimeError("cannot read Mach timebase")
    # rusage CPU counters are Mach absolute ticks, not nanoseconds on ARM64.
    ns_per_tick = base.numer / base.denom
    cpu_count = os.cpu_count() or 1
    identities = {}
    previous = {}
    previous_elapsed = None
    lost = set()
    started = time.monotonic()
    print(json.dumps({"kind": "metadata", "pids": args.pids, "host": args.host,
                      "ownership_method": "responsible_process_spi" if lookup else "explicit_ids",
                      "logical_cpus": cpu_count, "ns_per_tick": ns_per_tick,
                      "duration_s": args.duration, "interval_s": args.interval}), flush=True)
    while True:
        elapsed = time.monotonic() - started
        samples = []
        current = {}
        pids = responsible_pids(proc, lookup, args.host) if lookup else args.pids
        for pid in pids:
            usage = Usage()
            result = proc.proc_pid_rusage(pid, 2, ctypes.byref(usage))
            if lookup and lookup(pid) != args.host:
                samples.append({"pid": pid, "status": "ownership_changed"})
                continue
            if result or usage.exit or (pid in lost and not lookup):
                lost.add(pid)
                samples.append({"pid": pid, "status": "unavailable",
                                "errno": ctypes.get_errno() if result else None})
                continue
            identity = identities.setdefault(pid, usage.start)
            if identity != usage.start and (not lookup or pid == args.host):
                lost.add(pid)
                samples.append({"pid": pid, "status": "pid_reused"})
                continue
            # A new child may legitimately reuse an exited child's PID. The host
            # must keep its original identity, and responsibility is re-checked.
            identities[pid] = usage.start
            cpu_ns = round((usage.user + usage.system) * ns_per_tick)
            current[(pid, usage.start)] = cpu_ns
            samples.append({"pid": pid, "status": "sampled", "cpu_ns": cpu_ns,
                            "physical_footprint_bytes": usage.footprint,
                            "resident_bytes": usage.resident,
                            "disk_read_bytes": usage.disk_read,
                            "disk_write_bytes": usage.disk_write})
        if lookup and not any(identity[0] == args.host for identity in current):
            raise RuntimeError("host exited or its PID was reused; sampling stopped")
        complete = len(current) == len(pids)
        cpu_percent = None
        if complete and previous.keys() == current.keys() and previous_elapsed is not None:
            delta = sum(current[pid] - previous[pid] for pid in current)
            if delta >= 0 and elapsed > previous_elapsed:
                cpu_percent = delta / 1e9 / (elapsed - previous_elapsed) / cpu_count * 100
        print(json.dumps({"kind": "sample", "utc": datetime.datetime.now(
            datetime.timezone.utc).isoformat(), "elapsed_s": round(elapsed, 3),
            "specified_group_complete": complete,
            "ownership_verified": bool(lookup),
            "group_physical_footprint_bytes": sum(s["physical_footprint_bytes"]
                for s in samples) if complete else None,
            "group_cpu_capacity_percent": cpu_percent, "processes": samples}), flush=True)
        previous, previous_elapsed = current, elapsed
        if elapsed >= args.duration:
            break
        time.sleep(min(args.interval, args.duration - elapsed))


if __name__ == "__main__":
    main()
