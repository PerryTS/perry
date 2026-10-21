#!/usr/bin/env python3
"""Run a component host against a bounded, independently observed TCP/UDP peer."""
import argparse
import json
from pathlib import Path
import socket
import subprocess
import sys
import threading

EXPECTED_TCP = [f"tcp:{i}" for i in range(3)]
EXPECTED = [f"{mode}:{i}" for i in range(3) for mode in ("connected", "unconnected")]


def exercise(command, report: Path, timeout: float) -> int:
    received = []
    tcp_received = []
    errors = []
    stop = threading.Event()
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as peer, \
            socket.socket(socket.AF_INET, socket.SOCK_STREAM) as tcp_peer:
        peer.bind(("127.0.0.1", 0))
        peer.settimeout(0.2)
        port = peer.getsockname()[1]
        tcp_peer.bind(("127.0.0.1", 0))
        tcp_peer.listen()
        tcp_peer.settimeout(0.2)
        tcp_port = tcp_peer.getsockname()[1]

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

        def serve_tcp():
            while not stop.is_set():
                try:
                    connection, source = tcp_peer.accept()
                except socket.timeout:
                    continue
                except OSError as error:
                    errors.append(str(error))
                    return
                with connection:
                    connection.settimeout(2)
                    try:
                        data = b""
                        while not data.endswith(b"\n") and len(data) <= 256:
                            chunk = connection.recv(256)
                            if not chunk:
                                break
                            data += chunk
                        tcp_received.append({"payloadHex": data.hex(), "source": list(source)})
                        if not data.endswith(b"\n") or len(data) > 256:
                            errors.append("invalid TCP request framing")
                        else:
                            connection.sendall(b"reply:" + data)
                    except OSError as error:
                        errors.append(str(error))

        workers = [threading.Thread(target=target, daemon=True) for target in (serve, serve_tcp)]
        for worker in workers:
            worker.start()
        try:
            result = subprocess.run([*command, str(port), str(tcp_port)], timeout=timeout)
            status = result.returncode
        except (OSError, subprocess.TimeoutExpired) as error:
            errors.append(str(error))
            status = 124
        finally:
            stop.set()
            for worker in workers:
                worker.join(timeout=3)
        if any(worker.is_alive() for worker in workers):
            errors.append("network peer failed to stop")
    expected = [text.encode().hex() for text in EXPECTED]
    if [row["payloadHex"] for row in received] != expected:
        errors.append("UDP traffic did not match the six authorized exchanges")
    expected_tcp = [(text + "\n").encode().hex() for text in EXPECTED_TCP]
    if [row["payloadHex"] for row in tcp_received] != expected_tcp:
        errors.append("TCP traffic did not match the three authorized exchanges")
    passed = status == 0 and not errors
    report.write_text(json.dumps({"hostExit": status, "peerPort": port, "tcpPeerPort": tcp_port,
        "tcpReceived": tcp_received,
        "received": received, "errors": errors, "passed": passed}, indent=2) + "\n", encoding="utf-8")
    if not passed:
        print(f"FAIL network peer: host exited {status}; {errors}", file=sys.stderr)
        return 1
    print("PASS network peer: exactly six UDP and three TCP payloads observed", flush=True)
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
