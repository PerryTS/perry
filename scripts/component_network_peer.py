#!/usr/bin/env python3
"""Run a component host against a bounded, independently observed UDP peer."""
import argparse
import json
from pathlib import Path
import socket
import subprocess
import sys
import threading

EXPECTED = [f"{mode}:{i}" for i in range(3) for mode in ("connected", "unconnected")]


def exercise(command, report: Path, timeout: float) -> int:
    received = []
    errors = []
    stop = threading.Event()
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as peer:
        peer.bind(("127.0.0.1", 0))
        peer.settimeout(0.2)
        port = peer.getsockname()[1]

        def serve():
            while not stop.is_set():
                try:
                    data, source = peer.recvfrom(65536)
                    received.append({"payloadHex": data.hex(), "source": list(source)})
                    peer.sendto(b"reply:" + data, source)
                except socket.timeout:
                    continue
                except OSError as error:
                    errors.append(str(error))
                    return

        worker = threading.Thread(target=serve, daemon=True)
        worker.start()
        try:
            result = subprocess.run([*command, str(port)], timeout=timeout)
            status = result.returncode
        except (OSError, subprocess.TimeoutExpired) as error:
            errors.append(str(error))
            status = 124
        finally:
            stop.set()
            worker.join(timeout=2)
        if worker.is_alive():
            errors.append("UDP peer failed to stop")
    expected = [text.encode().hex() for text in EXPECTED]
    if [row["payloadHex"] for row in received] != expected:
        errors.append("UDP traffic did not match the six authorized exchanges")
    passed = status == 0 and not errors
    report.write_text(json.dumps({"hostExit": status, "peerPort": port,
        "received": received, "errors": errors, "passed": passed}, indent=2) + "\n", encoding="utf-8")
    if not passed:
        print(f"FAIL UDP peer: host exited {status}; {errors}", file=sys.stderr)
        return 1
    print("PASS UDP peer: exactly six authorized payloads observed", flush=True)
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--timeout", type=float, default=90)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command or args.timeout <= 0:
        parser.error("a command and a positive timeout are required")
    args.report.parent.mkdir(parents=True, exist_ok=True)
    return exercise(command, args.report, args.timeout)


if __name__ == "__main__":
    raise SystemExit(main())
