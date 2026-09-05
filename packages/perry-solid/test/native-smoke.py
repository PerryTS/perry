"""Exercise native-smoke.ts on macOS through its Geisterhand server."""

import argparse
import json
import socket
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("binary", type=Path)
parser.add_argument("--port", type=int, default=19764)
parser.add_argument("--output-dir", type=Path, required=True)
args = parser.parse_args()
if sys.platform != "darwin":
    parser.error("this test checks AppKit frame coordinates and requires macOS")
with socket.socket() as probe:
    probe.bind(("127.0.0.1", args.port))
args.output_dir.mkdir(parents=True, exist_ok=True)
base = f"http://127.0.0.1:{args.port}"


def get(path):
    with urllib.request.urlopen(base + path, timeout=4) as response:
        return response.read()


def wait_value(handle, expected):
    deadline = time.monotonic() + 8
    actual = None
    while time.monotonic() < deadline:
        actual = json.loads(get(f"/value/{handle}"))["value"]
        if actual == expected:
            return
        time.sleep(0.1)
    raise AssertionError((handle, expected, actual))


def click(handle):
    request = urllib.request.Request(base + f"/click/{handle}", method="POST", data=b"")
    with urllib.request.urlopen(request, timeout=4) as response:
        assert json.load(response)["ok"]


def capture(name):
    tree = get("/widgets?tree=true")
    (args.output_dir / f"{name}.json").write_bytes(tree)
    (args.output_dir / f"{name}.png").write_bytes(get("/screenshot"))
    return json.loads(tree)


with (args.output_dir / "stdout.log").open("wb") as stdout, (args.output_dir / "stderr.log").open("wb") as stderr:
    process = subprocess.Popen([str(args.binary.resolve())], stdout=stdout, stderr=stderr)
    try:
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError(f"app exited {process.returncode}; see stderr.log")
            try:
                if json.loads(get("/health"))["status"] == "ok":
                    buttons = sorted({item["handle"] for item in json.loads(get("/widgets?type=button"))
                                      if item["callback_kind"] == 0})
                    if len(buttons) == 4:
                        break
            except OSError:
                pass
            time.sleep(0.1)
        else:
            raise RuntimeError("Geisterhand and four smoke-test buttons did not start")

        before = capture("before")
        values = {item["handle"]: json.loads(get(f'/value/{item["handle"]}'))["value"] for item in before}
        by_text = {value: handle for handle, value in values.items() if value is not None}
        counter, raw = by_text["Count: 0"], by_text["Raw: 0"]
        rows = [by_text[name] for name in ("Alpha", "Beta", "Gamma")]
        increment, rotate, dispose, exit_button = buttons
        click(increment)
        wait_value(counter, "Count: 1")
        wait_value(raw, "Raw: 1")
        click(rotate)
        time.sleep(0.2)
        after = capture("after")
        frames = {item["handle"]: item["frame"] for item in after}
        # Same widget handles, now Gamma / Alpha / Beta, in AppKit's bottom-up coordinates.
        assert frames[rows[2]]["y"] > frames[rows[0]]["y"] > frames[rows[1]]["y"], frames
        click(dispose)
        time.sleep(0.2)
        wait_value(counter, "Count: 1")  # disposal also sets the signal to 99
        capture("disposed")
        try:
            click(exit_button)
        except OSError:
            pass  # process.exit can close the HTTP response first
        assert process.wait(timeout=8) == 0
        print("PASS native text identity, button events, keyed widget order, and disposal")
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=8)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
