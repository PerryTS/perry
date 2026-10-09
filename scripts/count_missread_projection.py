#!/usr/bin/env python3
"""External counts of class-role projection in the normal A/B executables."""
import argparse
from pathlib import Path
import shlex
import subprocess
from perf_missread_lane import PROGRAMS, environment

p = argparse.ArgumentParser()
p.add_argument("--hostdir", type=Path, required=True)
p.add_argument("--programs", default="qsstr,commander,zod5k")
opts = p.parse_args()
root = opts.hostdir.resolve()
for name in opts.programs.split(","):
    _, args, cwd = PROGRAMS[name]
    for arm in ["main", "head"]:
        binary = root / "measure" / arm / name
        text = "config = { missing_probes = warn; }\n"
        for function in ["is_anon_shape_class_id", "class_name_for_id",
                         "declared_class_outranks_anon_shape", "object_proto_id_for"]:
            text += (f'uprobe:{binary}:*{function} /pid == cpid/ '
                     f'{{ @calls["{function}"] = count(); }}\n')
        script = root / f"projection-{name}-{arm}.bt"
        script.write_text(text)
        _, _, env = environment(root, arm)
        with (root / f"projection-{name}-{arm}.txt").open("w") as out, \
                (root / f"projection-{name}-{arm}.err").open("w") as err:
            result = subprocess.run(["bpftrace", "-B", "line", "-c",
                                     shlex.join([str(binary), *args]), str(script)],
                                    cwd=root / cwd, env=env, stdout=out, stderr=err, timeout=1800)
        if result.returncode:
            raise RuntimeError(f"{name}/{arm}: bpftrace exit {result.returncode}")
        print(f"{name}/{arm}: projection counts complete", flush=True)
