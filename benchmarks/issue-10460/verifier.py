#!/usr/bin/env python3
"""Compare exact Node-API verifier bodies using macOS CPU/RSS measurements."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import statistics
import subprocess
import tempfile


def function(source, name):
    start = source.index("fn " + name + "(")
    cursor = source.index("{", start) + 1
    depth = 1
    while depth:
        depth += (source[cursor] == "{") - (source[cursor] == "}")
        cursor += 1
    return source[start:cursor]


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--baseline", default="9e29f59d43")
parser.add_argument("--tiny", action="store_true", help="16 KiB unchanged payload, 10,000 verifications per process")
args = parser.parse_args()
repo = Path(__file__).resolve().parents[2]
source_path = "crates/perry-runtime/src/node_api_host/loader.rs"
baseline = subprocess.check_output(["git", "show", f"{args.baseline}:{source_path}"], text=True, cwd=repo)
candidate = (repo / source_path).read_text()
iterations = 10000 if args.tiny else 40
common = r'''use std::path::{Path,PathBuf}; use std::io::Read; use sha2::{Sha256,Digest};
struct ManifestFile {path:String,sha256:String,size:u64}
struct ManifestAddon {logical_id:String,entry:String,files:Vec<ManifestFile>}
fn main(){let args:Vec<String>=std::env::args().collect(); let root=Path::new(&args[1]);
let list=std::fs::read_to_string(&args[2]).unwrap();
let files=list.lines().map(|line|{let fields:Vec<_>=line.split('\t').collect();
ManifestFile{path:fields[0].into(),sha256:fields[1].into(),size:fields[2].parse().unwrap()}}).collect();
let addon=ManifestAddon{logical_id:"fixture/addon.node".into(),entry:"addon.node".into(),files};
for _ in 0..ITERATIONS {std::hint::black_box(verify_addon_payload(root,&addon).unwrap());}}
'''.replace("ITERATIONS", str(iterations))

with tempfile.TemporaryDirectory(prefix="perry-napi-verifier-") as scratch:
    work = Path(scratch)
    (work / "Cargo.toml").write_text(f'''[package]
name="napi-verifier-perf"
version="0.1.0"
edition="2021"
[dependencies]
sha2="=0.11.0"
perry-hex={{path="{repo / 'crates/perry-hex'}"}}
[[bin]]
name="baseline"
path="baseline.rs"
[[bin]]
name="candidate"
path="candidate.rs"
''')
    for name, source in [("baseline", baseline), ("candidate", candidate)]:
        (work / f"{name}.rs").write_text(common + function(source, "safe_payload_path") + "\n" + function(source, "verify_addon_payload"))
    subprocess.run(["cargo", "build", "--release", "--manifest-path", str(work / "Cargo.toml")], check=True)
    payload = work / "payload"
    payload.mkdir()
    sizes = [("addon.node", 16 * 1024)] if args.tiny else [
        ("addon.node", 2 * 1024 * 1024), ("sqlite3.c", 10 * 1024 * 1024), ("foreign.node", 16 * 1024 * 1024)]
    files = []
    for name, size in sizes:
        data = bytes(range(256)) * (size // 256)
        (payload / name).write_bytes(data)
        files.append((name, hashlib.sha256(data).hexdigest(), len(data)))
    selections = {"baseline": files, "candidate": files[:1]}
    if not args.tiny:
        selections["candidate-full"] = files
    for name, selection in selections.items():
        (work / f"{name}.tsv").write_text("".join(f"{path}\t{sha}\t{size}\n" for path, sha, size in selection))
    samples = {name: [] for name in selections}
    for trial in range(7 if args.tiny else 5):
        # Alternate order to avoid attributing concurrent load drift to an arm.
        order = list(selections) if trial % 2 == 0 else list(reversed(selections))
        for name in order:
            binary = "baseline" if name == "baseline" else "candidate"
            result = subprocess.run(["/usr/bin/time", "-l", str(work / "target/release" / binary),
                                     str(payload), str(work / f"{name}.tsv")], text=True, capture_output=True, check=True)
            timing = re.search(r"([\d.]+) real\s+([\d.]+) user\s+([\d.]+) sys", result.stderr)
            rss = re.search(r"(\d+)\s+maximum resident set size", result.stderr)
            user, system = float(timing[2]), float(timing[3])
            samples[name].append({"user": user, "system": system, "cpu": user + system, "rss": int(rss[1])})
    for name, values in samples.items():
        medians = {key: statistics.median(sample[key] for sample in values) for key in values[0]}
        print(name, json.dumps({"medians": medians, "samples": values}), flush=True)
