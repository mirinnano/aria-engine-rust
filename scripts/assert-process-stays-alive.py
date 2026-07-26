#!/usr/bin/env python3
"""Assert that a GUI smoke process launches and remains healthy briefly.

Native visual-novel players are event loops: reaching an authored `end` state
does not necessarily mean that a window should immediately exit. Some desktop
backends retain the event loop; others close cleanly after the authored end.
This helper rejects an early exit or any non-zero exit, but can accept a clean
completion after a short startup interval. It reliably terminates a process
group that remains open so CI never leaves a window or Xvfb behind.
"""

from __future__ import annotations

import argparse
import os
import signal
import subprocess
import sys
import time


def stop_process(process: subprocess.Popen[object]) -> None:
    if process.poll() is not None:
        return
    if os.name == "posix":
        os.killpg(process.pid, signal.SIGTERM)
    else:
        process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        if os.name == "posix":
            os.killpg(process.pid, signal.SIGKILL)
        else:
            process.kill()
        process.wait(timeout=5)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seconds", type=float, default=4.0)
    parser.add_argument(
        "--minimum-seconds",
        type=float,
        help="accept a clean exit only after this many seconds (defaults to --seconds)",
    )
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if args.seconds <= 0:
        parser.error("--seconds must be positive")
    minimum_seconds = args.seconds if args.minimum_seconds is None else args.minimum_seconds
    if minimum_seconds <= 0:
        parser.error("--minimum-seconds must be positive")
    if minimum_seconds > args.seconds:
        parser.error("--minimum-seconds cannot exceed --seconds")
    if not command:
        parser.error("a command is required after --")

    process = subprocess.Popen(command, start_new_session=os.name == "posix")
    try:
        started = time.monotonic()
        deadline = started + args.seconds
        while time.monotonic() < deadline:
            exit_code = process.poll()
            if exit_code is not None:
                elapsed = time.monotonic() - started
                if exit_code == 0 and elapsed >= minimum_seconds:
                    print(
                        f"launch smoke passed: {' '.join(command)} completed cleanly after {elapsed:.2f}s"
                    )
                    return 0
                print(
                    f"launch smoke failed: {' '.join(command)} exited after {elapsed:.2f}s with {exit_code}",
                    file=sys.stderr,
                )
                return 1
            time.sleep(0.05)
        print(f"launch smoke passed: {' '.join(command)} stayed alive for {args.seconds:g}s")
        return 0
    finally:
        stop_process(process)


if __name__ == "__main__":
    raise SystemExit(main())
