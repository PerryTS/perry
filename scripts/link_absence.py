#!/usr/bin/env python3
"""HWS: linked-symbol absence, live positive witnesses, and retainer diagnostics.

Run after build-perry and gc_call_effects/sidecars.sh. Requires the pinned Node,
LLVM nm/cxxfilt, and (on ELF) LLVM 22 lld. Every RUNTIME_INSTALLS hook must own a
fixture; adding a hook without coverage fails closed. Rules are fixture comments,
not another runtime feature inventory. Unhooked rules explicitly say so.

--record-known-failures writes the initial HWS2 symbol exceptions for this host.
Review that diff: it is never used by CI. Removing the last leak requires removing
its exception, so subsequent regressions fail. New symbols fail even within an
already leaking subsystem. Existing exceptions are names, never byte budgets.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import re
import shutil
import struct
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "tests/link_absence"


def run(cmd, *, cwd=None, env=None, timeout=180):
    return subprocess.run([str(x) for x in cmd], cwd=cwd, env=env,
                          capture_output=True, timeout=timeout)


def checked(cmd, **kwargs):
    result = run(cmd, **kwargs)
    if result.returncode:
        raise RuntimeError(f"{cmd[0]} exited {result.returncode}: "
                           + (result.stdout + result.stderr).decode(errors="replace")[-4000:])
    return result.stdout.decode(errors="replace")


def tool(name):
    prefix = os.environ.get("LLVM_SYS_221_PREFIX")
    candidate = Path(prefix)/"bin"/name if prefix else None
    value = (os.environ.get(name.upper().replace("-", "_"))
             or (str(candidate) if candidate and candidate.is_file() else None)
             or shutil.which(name))
    if not value:
        raise RuntimeError(f"required tool {name} missing (use setup-llvm22)")
    return value


def inventory():
    source = (ROOT / "crates/perry/src/commands/stdlib_installs.rs").read_text()
    section = source.split("const RUNTIME_INSTALLS:", 1)[1].split("\n];", 1)[0]
    hooks = set(re.findall(r'"(js_runtime_install_\w+)"', section))
    assert hooks, "RUNTIME_INSTALLS not found"
    fixtures = {}
    for path in sorted(FIXTURES.glob("*.ts")):
        if path.stem in ("hello", "without_math"):
            continue
        source = path.read_text()
        hook = re.search(r"^// HWS hook: (.+)$", source, re.M)
        rule = re.search(r"^// HWS prefixes: (.+)$", source, re.M)
        assert hook and rule, f"{path}: missing hook/prefix declaration"
        hook = hook[1]
        assert hook in hooks or hook == "none (no hook yet)", f"{path}: unknown hook {hook}"
        prefixes = rule[1].split("|")
        assert all(prefixes), f"{path}: empty prefix matches every symbol"
        if hook in hooks:
            prefixes.append(hook)
        fixtures[path.stem] = {"hook": hook, "prefixes": prefixes}
    covered = {f["hook"] for f in fixtures.values()}
    assert hooks <= covered, f"new runtime hooks need positive witnesses: {sorted(hooks-covered)}"
    return fixtures


def text_bounds(binary):
    """Read the exact executable text section bounds, including Mach-O padding."""
    data = binary.read_bytes()
    if data[:4] == b"\x7fELF":
        assert data[4:6] == b"\x02\x01", "expected little-endian ELF64"
        shoff = struct.unpack_from("<Q", data, 40)[0]
        entsize, count, names_index = struct.unpack_from("<HHH", data, 58)
        headers = [struct.unpack_from("<IIQQQQIIQQ", data, shoff+i*entsize) for i in range(count)]
        strings = headers[names_index]
        names = data[strings[4]:strings[4]+strings[5]]
        for row in headers:
            if names[row[0]:].split(b"\0",1)[0] == b".text":
                return row[3], row[3]+row[5]
    elif data[:4] == b"\xcf\xfa\xed\xfe":
        commands = struct.unpack_from("<I", data, 16)[0]
        offset = 32
        for _ in range(commands):
            command, size = struct.unpack_from("<II", data, offset)
            if command == 0x19:  # LC_SEGMENT_64
                nsects = struct.unpack_from("<I", data, offset+64)[0]
                for index in range(nsects):
                    section = offset+72+index*80
                    name, segment, address, length = struct.unpack_from("<16s16sQQ", data, section)
                    if name.rstrip(b"\0") == b"__text" and segment.rstrip(b"\0") == b"__TEXT":
                        return address, address+length
            offset += size
    raise RuntimeError(f"cannot locate text section in {binary}")


def canonical(name):
    return re.sub(r"::h[0-9a-f]{16}$", "", name)


def symbols(binary, nm, demangler):
    lo, hi = text_bounds(binary)
    # Infer spans by address on both formats; Mach-O nlist has no st_size.
    lines = checked([nm, "--format=posix", "--numeric-sort", "--defined-only", binary]).splitlines()
    rows = []
    for line in lines:
        fields = line.split()
        if len(fields) < 3 or fields[1] not in ("T", "t"):
            continue
        address = int(fields[2], 16)
        if lo <= address < hi:
            raw = fields[0]
            # Mach-O's platform underscore is outside the Rust mangling.
            demangle = raw[1:] if sys.platform == "darwin" else raw
            rows.append({"raw": raw, "input": demangle, "address": address})
    assert rows, f"{binary}: no text symbols; PERRY_KEEP_SYMBOLS must be effective"
    result = subprocess.run([demangler, "--format=rust"],
                            input="\n".join(r["input"] for r in rows)+"\n",
                            text=True, capture_output=True, check=True)
    names = result.stdout.splitlines()
    assert len(names) == len(rows), "demangler lost symbols"
    addresses = sorted({r["address"] for r in rows} | {hi})
    spans = {a: b-a for a,b in zip(addresses, addresses[1:])}
    for row, name in zip(rows, names):
        row["name"] = canonical(name)
        row["bytes"] = spans[row["address"]]
    return rows, hi-lo


def matches(symbol, fixture):
    # Rust trait impl names start with '<'; their defining module still counts.
    name = symbol["name"].lstrip("<")
    return any(name.startswith(prefix) for prefix in fixture["prefixes"])


def compile_witness(args, name, env, extra=""):
    work = args.output / name
    work.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(FIXTURES / (name+".ts"), work / "main.ts")
    shutil.copyfile(FIXTURES / "value.json", work / "value.json")
    binary = work / "app"
    if name == "node_api_host":
        package = work / "node_modules/hws-addon"
        package.mkdir(parents=True, exist_ok=True)
        (work / "package.json").write_text(json.dumps({"private": True, "perry": {
            "compilePackages": ["hws-addon"], "allow": {"compilePackages": ["hws-addon"]},
            "nativeAddons": ["hws-addon"]}}))
        (package / "package.json").write_text('{"name":"hws-addon","version":"1.0.0","main":"index.js"}')
        (package / "index.js").write_text('module.exports = require("./addon.node");\n')
        cmd = [tool("clang"), "-shared", "-fPIC",
               ROOT / "crates/perry/tests/fixtures/node_api_host/addon.c", "-o", package / "addon.node"]
        if sys.platform == "darwin":
            cmd += ["-undefined", "dynamic_lookup"]
        checked(cmd)
    compile_env = {**env, "PERRY_KEEP_SYMBOLS": "1"}
    if extra:
        compile_env["PERRY_EXTRA_LINK_ARGS"] = extra
    result = run([args.perry, "compile", work/"main.ts", "--no-cache",
                  "--keep-intermediates", "-v", "-o", binary], cwd=work, env=compile_env, timeout=600)
    log = result.stdout + result.stderr
    (work / ("why-live.log" if extra else "compile.log")).write_bytes(log)
    if result.returncode:
        raise RuntimeError(f"{name}: compile exited {result.returncode}; {work}/compile.log\n"
                           + log.decode(errors="replace")[-3000:])
    assert b"using proven tiny program:" not in log, f"{name}: tiny path invalidates this gate"
    expected_archive = str(Path(env["PERRY_RUNTIME_DIR"])/"libperry_runtime.a").encode()
    assert expected_archive in log, f"{name}: compile did not link the pinned runtime archive"
    installs = re.findall(rb"runtime installs: ([^\r\n]*)", log)
    assert len(installs) == 1, f"{name}: missing selected runtime install evidence"
    selected = set(installs[0].decode().split(", ")) - {""}
    assert "js_runtime_install_compiled" not in selected, "full installs invalidate absence check"
    return binary, selected, log


def why_live(args, name, symbol, env, demangler):
    raw = symbol["raw"]
    if sys.platform == "darwin":
        # Apple's final symbol table drops LTO's .llvm.<id> suffix. why_live
        # needs the input atom spelling, not that renamed output symbol.
        archive = Path(env["PERRY_RUNTIME_DIR"])/"libperry_runtime.a"
        listing = checked([tool("llvm-nm"), "--format=posix", "--defined-only", archive])
        candidates = [line.split()[0] for line in listing.splitlines()
                      if line.split() and line.split()[0].startswith(raw+".llvm.")]
        if candidates:
            raw = candidates[0]
        flags = f"-Wl,-why_live,{raw}"
    else:
        flags = f"-fuse-ld=lld -Wl,--why-live={raw}"
    symbol["linker_raw"] = raw
    _, _, log = compile_witness(args, name, env, flags)
    # Keep the full build log on disk, but report only the linker's chain.
    text = log.decode(errors="replace")
    pattern = (r"(?m)^"+re.escape(raw)+r" from .*(?:\n +.*)*"
               if sys.platform == "darwin" else r"(?m)^live symbol:.*(?:\n>>>.*)*")
    found = re.search(pattern, text)
    assert found, "linker returned no why-live chain; see why-live.log"
    chain = found[0] + "\n"
    if sys.platform == "darwin":
        chain = re.sub(r"(?<!\w)_(?=_(?:ZN|R))", "", chain)
    result = subprocess.run([demangler, "--format=rust"], input=chain.encode(),
                            capture_output=True, check=True)
    output = result.stdout.decode(errors="replace")
    (args.output/name/"why-live.demangled.log").write_text(output)
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--perry", default=os.environ.get("PERRY_BIN", ROOT/"target/release/perry"))
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--record-known-failures", action="store_true")
    parser.add_argument("--negative-only", action="store_true", help="diagnostic/sabotage arm; CI always runs all positives")
    args = parser.parse_args()
    args.perry = str(Path(args.perry).resolve())
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    fixtures = inventory()
    nm, demangler = tool("llvm-nm"), tool("llvm-cxxfilt")
    node_version = checked(["node", "--version"]).strip()
    assert node_version == "v"+(ROOT/".node-version").read_text().strip(), f"wrong Node oracle {node_version}"
    key = f"{sys.platform}-{platform.machine().lower()}"
    known_path = FIXTURES / "known-failing" / (key+".json")
    known = json.loads(known_path.read_text()) if known_path.exists() else {"lane": "HWS2", "symbols": {}}
    assert known["lane"] == "HWS2", "unknown exception owner"
    assert set(known["symbols"]) <= set(fixtures), "exceptions for removed subsystem rules"
    env = dict(os.environ)
    env["PATH"] = str(Path(nm).parent) + os.pathsep + env.get("PATH", "")
    env.setdefault("PERRY_RUNTIME_DIR", str(Path(args.perry).parent))
    report = {"platform": key, "node": node_version, "witnesses": {}, "subsystems": {}, "failures": []}
    exceptions = {}
    names = ["hello", "without_math"] + ([] if args.negative_only else list(fixtures))
    for name in names:
        try:
            binary, selected, _ = compile_witness(args, name, env)
            oracle = run(["node", args.output/name/"main.ts", "world"], cwd=binary.parent)
            actual = run([binary, "world"], cwd=binary.parent)
            (binary.parent/"node.stdout").write_bytes(oracle.stdout)
            (binary.parent/"native.stdout").write_bytes(actual.stdout)
            (binary.parent/"node.stderr").write_bytes(oracle.stderr)
            (binary.parent/"native.stderr").write_bytes(actual.stderr)
            assert oracle.returncode == 0, f"{name}: Node oracle failed: {oracle.stderr.decode(errors='replace')}"
            assert (actual.returncode, actual.stdout) == (oracle.returncode, oracle.stdout), (
                f"{name}: Node parity failed: native exit={actual.returncode}, "
                f"stdout={actual.stdout!r}, expected={oracle.stdout!r}; stderr={actual.stderr[-1000:]!r}")
            syms, text = symbols(binary, nm, demangler)
            report["witnesses"][name] = {"text_bytes": text, "binary_bytes": binary.stat().st_size,
                                          "installs": sorted(selected), "parity": True}
            if name not in ("hello", "without_math"):
                fixture = fixtures[name]
                assert any(matches(s, fixture) for s in syms), f"{name}: positive linked no matching symbols"
                if fixture["hook"].startswith("js_"):
                    assert fixture["hook"] in selected, f"{name}: positive did not install {fixture['hook']}"
            else:
                candidates = fixtures.items() if name == "hello" else [("global_math", fixtures["global_math"])]
                for subsystem, fixture in candidates:
                    hit = [s for s in syms if matches(s, fixture)]
                    installed = fixture["hook"] in selected
                    if name == "without_math":
                        assert not installed, "Math's negative witness unexpectedly installs Math"
                    observed = sorted({s["name"] for s in hit}) if not installed else []
                    allowed = known["symbols"].get(subsystem, [])
                    new = sorted(set(observed)-set(allowed))
                    failure = not installed and bool(new)
                    stale = not installed and bool(allowed) and not observed
                    row = {"hook": fixture["hook"], "installed": installed, "linked": bool(hit),
                           "bytes": sum({s["address"]:s["bytes"] for s in hit}.values()),
                           "symbols": len(observed), "new_symbols": new,
                           "status": "installed" if installed else "HWS2" if observed and not failure else "FAIL" if failure else "absent"}
                    if name == "hello":
                        report["subsystems"][subsystem] = row
                    else:
                        report["math_negative"] = row
                    if observed:
                        exceptions[subsystem] = observed
                    if hit and not installed and (args.record_known_failures or failure):
                        largest = max(hit, key=lambda s:s["bytes"])
                        row["largest"] = largest
                        row["why_live"] = why_live(args, name, largest, env, demangler)
                        # Retain a separate chain per rule (the compile log is reused).
                        (args.output/f"why-{subsystem}.log").write_text(row["why_live"])
                        if failure and not args.record_known_failures:
                            print(f"FAIL {subsystem}: new linked symbols {new[:3]}; largest={largest['name']}\n{row['why_live']}", flush=True)
                    if failure and not args.record_known_failures:
                        report["failures"].append(f"{subsystem}: uninstalled subsystem has new linked symbols")
                    if stale and not args.record_known_failures:
                        report["failures"].append(f"{subsystem}: fixed; remove HWS2 exception to lock in absence")
            print(f"{name}: parity ok, text={text}, installs={len(selected)}", flush=True)
        except (AssertionError, RuntimeError, subprocess.SubprocessError) as error:
            report["failures"].append(str(error))
            print(f"FAIL {error}", flush=True)
    if args.record_known_failures and not report["failures"]:
        assert "hello" in report["witnesses"], "cannot record exceptions from a broken hello"
        known_path.parent.mkdir(parents=True, exist_ok=True)
        known_path.write_text(json.dumps({"lane":"HWS2", "symbols":exceptions}, indent=2, sort_keys=True)+"\n")
    report["elapsed_seconds"] = round(time.monotonic()-started, 2)
    (args.output/"report.json").write_text(json.dumps(report, indent=2, sort_keys=True)+"\n")
    print(f"HWS elapsed={report['elapsed_seconds']}s failures={len(report['failures'])}")
    return int(bool(report["failures"]))


if __name__ == "__main__":
    sys.exit(main())
