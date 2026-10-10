"""Prepare an unpack benchmark without changing the upstream worker sources."""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import shutil

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("upm_source", type=Path)
parser.add_argument("corpus", type=Path)
parser.add_argument("destination", type=Path)
args = parser.parse_args()
args.destination.mkdir(parents=True, exist_ok=True)
source_hashes = {}
for source in sorted(args.upm_source.glob("*.ts")):
    shutil.copyfile(source, args.destination / source.name)
    source_hashes[source.name] = hashlib.sha256(source.read_bytes()).hexdigest()
assert "unpack-worker.ts" in source_hashes
shutil.copyfile(Path(__file__).with_name("micro.ts"), args.destination / "micro.ts")
(args.destination / "package.json").write_text('{"type":"module"}\n')
manifest = json.loads(Path(__file__).with_name("corpus.json").read_text())
files, integrities = [], []
for i, item in enumerate(manifest):
    stem = args.corpus.resolve() / f"{i:02d}"
    compressed = stem.with_suffix(".tgz").read_bytes()
    raw = stem.with_suffix(".tar").read_bytes()
    assert hashlib.sha256(compressed).hexdigest() == item["sha256"]
    assert len(compressed) == item["compressed"] and len(raw) == item["inflated"]
    files.append(str(stem))
    integrities.append("sha512-" + base64.b64encode(hashlib.sha512(compressed).digest()).decode())
assert len(files) == 22 and manifest[1]["compressed"] == 8075816
(args.destination / "files.ts").write_text("export const files = " + json.dumps(files) + ";\n")
(args.destination / "integrities.ts").write_text("export const integrities = " + json.dumps(integrities, separators=(',', ':')) + ";\n")
(args.destination / "source-hashes.json").write_text(json.dumps(source_hashes, indent=2) + "\n")
print(f"Prepared {len(files)} tarballs and {len(source_hashes)} unchanged source modules")
