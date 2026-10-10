#!/usr/bin/env python3
"""Collect external per-site counts from an accepted symbol-preserving binary.

Diagnostic binaries are for causality, never for the instructions/RSS A/B.
The parent is pinned to CPUs 0-55 with ASLR disabled by the caller.
"""
import argparse
import re
from pathlib import Path
import shlex
import subprocess
from perf_missread_lane import PROGRAMS, environment

p = argparse.ArgumentParser()
p.add_argument("--hostdir", type=Path, required=True)
p.add_argument("--arm", choices=["main", "head"], required=True)
p.add_argument("--programs", default="qsparse,fastify,tsc")
p.add_argument("--normal", action="store_true", help="probe the normal A/B executable")
p.add_argument("--binary-dir", type=Path,
               help="probe identical private ELF copies without attaching to A/B processes")
p.add_argument("--key-buffers", action="store_true",
               help="use bounded byte keys when bpftrace string lowering fails; decode at NUL")
opts = p.parse_args()
root = opts.hostdir.resolve()
template = Path(__file__).with_name("count_missread.bt.in").read_text()
for name in opts.programs.split(","):
    _, args, cwd = PROGRAMS[name]
    binary = (opts.binary_dir / name if opts.binary_dir else
              root / "measure" / (opts.arm if opts.normal else opts.arm + "-diag") / name)
    # Probe XMM0 after its exact transfer to RAX; never infer the receiver
    # from a nested lookup or a later priming call. Fail if this ABI witness changes.
    symbols = subprocess.check_output(["nm", str(binary)], text=True)
    address = int(next(line.split()[0] for line in symbols.splitlines()
                       if line.endswith(" js_method_site_miss")), 16)
    entry = subprocess.check_output(["objdump", "-d", f"--start-address={address}",
                                     f"--stop-address={address + 5}", str(binary)], text=True)
    if "66 48 0f 7e c0" not in entry or "%xmm0,%rax" not in entry:
        raise RuntimeError(f"{binary}: method receiver probe instruction changed")
    script = root / f"{name}.{opts.arm}.bt"
    script_text = template.replace("@BINARY@", str(binary))
    if opts.key_buffers:
        script_text = script_text.replace("str(arg1 + 20, *(uint32*)uptr(arg1 + 4) + 1)",
                                          "buf(arg1 + 20, 32)")
        script_text = script_text.replace("str(arg0 + 20, *(uint32*)uptr(arg0 + 4) + 1)",
                                          "buf(arg0 + 20, 32)")
        script_text = script_text.replace("str(*(uint64*)uptr($descriptor + 16),\n      *(uint32*)uptr($descriptor) + 1)",
                                          "buf(*(uint64*)uptr($descriptor + 16), 32)")
        script_text = re.sub(r'  if \(buf\(arg1.*?  @cache_state', '  @cache_state',
                             script_text, flags=re.S)
        script_text = script_text.replace('  print(@prototype_bags, 40);', '').replace('  clear(@prototype_bags);', '')
    if not any(line.endswith(" js_put_value_set_packed_miss") for line in symbols.splitlines()):
        script_text = re.sub(r'uprobe:[^\n]*:js_put_value_set_packed_miss .*?\n}', '',
                             script_text, flags=re.S)
        script_text = script_text.replace('  print(@stores);', '').replace('clear(@stores);', '')
    script.write_text(script_text)
    flags = ["PERRY_METHOD_SITE_STATS=1"]
    if not opts.normal:
        flags.append(f"PERRY_IC_DIAG={root / (name + '.' + opts.arm + '.ic.txt')}")
    command = shlex.join(["env", *flags, str(binary), *args])
    _, _, env = environment(root, opts.arm)
    label = "before" if opts.arm == "main" else "after"
    with (root / f"{name}.uprobes.{label}.txt").open("w") as out, \
            (root / f"{name}.uprobes.{label}.err").open("w") as err:
        result = subprocess.run(["bpftrace", "-B", "line", "-c", command, str(script)],
                                cwd=root / cwd, env=env, stdout=out, stderr=err, timeout=2700)
    if result.returncode:
        raise RuntimeError(f"{name}: bpftrace exit {result.returncode}")
    print(f"{opts.arm}/{name}: probes complete", flush=True)
