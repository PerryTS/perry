#!/usr/bin/env python3
"""Ratchet duplicate unmangled exports in runtime, stdlib and native bindings.

Source inventory (all features/targets), not a claim that every recorded pair
co-links. Existing feature-gated providers are recorded by symbol AND location;
a new provider or a baseline increase fails. Macro-generated exports count.
No build products, runtime storage, or dependency resolution are changed.
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASELINE = "scripts/native_export_collisions.json"
# Strings remain indivisible tokens so prose cannot manufacture attributes.
TOKEN = re.compile(r'(?:b|c)?r(\#*)"[\s\S]*?"\1|"(?:[^"\\]|\\[\s\S])*"|\'(?:[^\'\\]|\\(?:u\{[0-9A-Fa-f_]+\}|x[0-9A-Fa-f]{2}|[\s\S]))\'|[A-Za-z_][\w]*|[^\s]')


def tokens(source):
    out, i = [], 0
    while i < len(source):
        if source.startswith("//", i):
            end = source.find("\n", i)
            i = len(source) if end < 0 else end
        elif source.startswith("/*", i):
            depth, i = 1, i + 2
            while i < len(source) and depth:
                if source.startswith("/*", i):
                    depth, i = depth + 1, i + 2
                elif source.startswith("*/", i):
                    depth, i = depth - 1, i + 2
                else:
                    i += 1
            if depth:
                raise ValueError("unterminated block comment")
        elif source[i].isspace():
            i += 1
        else:
            m = TOKEN.match(source, i)
            if not m:
                raise ValueError(f"cannot tokenize source at {i}")
            out.append(m.group())
            i = m.end()
    return out


def closing(ts, i):
    ends = {"(": ")", "[": "]", "{": "}"}
    stack = [ends[ts[i]]]
    for j in range(i + 1, len(ts)):
        if ts[j] in ends:
            stack.append(ends[ts[j]])
        elif ts[j] in ends.values():
            if not stack or ts[j] != stack.pop():
                raise ValueError("unbalanced token tree")
            if not stack:
                return j
    raise ValueError("unterminated token tree")


def cfg_without_tests(expr):
    """Three-valued cfg evaluation: test=false, platform/features unknown."""
    if expr == ["test"]:
        return False
    if len(expr) >= 3 and expr[1] == "(" and expr[-1] == ")":
        parts, start, i = [], 2, 2
        while i < len(expr) - 1:
            if expr[i] in ("(", "[", "{"):
                i = closing(expr, i) + 1
                continue
            if expr[i] == ",":
                if i > start:
                    parts.append(cfg_without_tests(expr[start:i]))
                start = i + 1
            i += 1
        if start < len(expr) - 1:
            parts.append(cfg_without_tests(expr[start:-1]))
        if expr[0] == "not" and len(parts) == 1:
            return None if parts[0] is None else not parts[0]
        if expr[0] == "all":
            return False if False in parts else (True if all(v is True for v in parts) else None)
        if expr[0] == "any":
            return True if True in parts else (False if all(v is False for v in parts) else None)
    return None


def production_tokens(ts):
    """Remove cfg(test) items, including inline modules and test-only stubs."""
    out, test_modules, i = [], [], 0
    while i < len(ts):
        if ts[i:i + 2] == ["#", "["]:
            end = closing(ts, i + 1)
            if ts[i + 2:i + 4] == ["cfg", "("] and cfg_without_tests(ts[i + 4:end - 1]) is False:
                j = end + 1
                while j < len(ts):
                    if ts[j:j + 2] == ["#", "["]:
                        j = closing(ts, j + 1) + 1
                        continue
                    if ts[j] == "mod" and j + 2 < len(ts) and ts[j + 2] == ";":
                        test_modules.append(ts[j + 1])
                    if ts[j] == "{":
                        j = closing(ts, j) + 1
                        break
                    if ts[j] == ";":
                        j += 1
                        break
                    if ts[j] in ("(", "["):
                        j = closing(ts, j) + 1
                    else:
                        j += 1
                i = j
                continue
        out.append(ts[i])
        i += 1
    return out, test_modules


def macro_definitions(ts):
    definitions, masked, i = {}, list(ts), 0
    while i + 3 < len(ts):
        if ts[i:i + 2] == ["macro_rules", "!"]:
            name, start = ts[i + 2], i + 3
            end = closing(ts, start)
            definitions[name] = ts[start + 1:end]
            masked[i:end + 1] = [";"] * (end + 1 - i)
            i = end + 1
        else:
            i += 1
    return definitions, masked


def literal_exports(ts):
    exports, i, export = [], 0, None
    while i < len(ts):
        if ts[i:i + 2] == ["#", "["]:
            end = closing(ts, i + 1)
            attr = ts[i + 2:end]
            if "no_mangle" in attr:
                export = True
            if "export_name" in attr:
                j = attr.index("export_name")
                if attr[j + 1:j + 2] != ["="] or not attr[j + 2].startswith('"'):
                    raise ValueError("nonliteral export_name requires scanner support")
                export = json.loads(attr[j + 2])
            i = end + 1
            continue
        if export is not None and ts[i] in ("fn", "static", "const"):
            j = i + 1
            if ts[j] == "mut":
                j += 1
            name = "$" + ts[j + 1] if ts[j] == "$" else ts[j]
            exports.append(name if export is True else export)
            export = None
        elif ts[i] in (";", "{", "}"):
            export = None
        i += 1
    return exports


def source_exports(sources):
    """A crate's literal and name-argument macro exports, with source paths.

    Export macros in the tree use a single name argument. Unsupported shapes
    fail rather than silently losing the generated symbol from the inventory.
    """
    all_defs, all_macros, bodies, parsed, excluded = {}, {}, {}, {}, set()
    for path, source in sources.items():
        parsed[path], test_modules = production_tokens(tokens(source))
        parent = Path(path).parent if Path(path).stem in ("lib", "mod") else Path(path).with_suffix("")
        for name in test_modules:
            module = parent / name
            excluded.update(p for p in sources if Path(p) == module.with_suffix(".rs") or module in Path(p).parents)
    for path, ts in parsed.items():
        if path in excluded:
            continue
        definitions, body = macro_definitions(ts)
        bodies[path] = body
        all_macros[path] = definitions
        for name, definition in definitions.items():
            generated = literal_exports(definition)
            if not generated:
                continue
            params = set(re.findall(r"\$ (\w+) : ident", " ".join(definition)))
            if any(g.startswith("$") and g[1:] not in params for g in generated):
                raise ValueError(f"{path}: unsupported exporting macro {name}")
            # The first ident matcher is the exported name in current macros.
            first = next((m for m in re.finditer(r"\$ (\w+) : (\w+)", " ".join(definition)) if m.group(2) != "meta"), None)
            if any(g.startswith("$") and (first is None or g[1:] != first.group(1) or first.group(2) != "ident") for g in generated):
                raise ValueError(f"{path}: {name} exports a non-first ident parameter")
            signature = tuple(sorted(set(generated)))
            if name in all_defs and all_defs[name] != signature:
                raise ValueError(f"{path}: conflicting export macro {name}")
            all_defs[name] = signature
    for path, definitions in all_macros.items():
        for name, definition in definitions.items():
            if name not in all_defs and any(definition[i] in all_defs and definition[i + 1] == "!"
                                            for i in range(len(definition) - 1)):
                raise ValueError(f"{path}: wrapper macro {name} invokes an export macro; add expansion support")
    out = defaultdict(set)
    for path, ts in bodies.items():
        for name in literal_exports(ts):
            if name.startswith("$"):
                raise ValueError(f"{path}: unexpanded export {name}")
            out[name].add(path)
        i = 0
        while i + 2 < len(ts):
            if ts[i] in all_defs and ts[i + 1] == "!" and ts[i + 2] in ("(", "{", "["):
                end = closing(ts, i + 2)
                args = ts[i + 3:end]
                # Skip attributes (including doc attributes) preceding the name.
                while args[:2] == ["#", "["]:
                    args = args[closing(args, 1) + 1:]
                if not args or not re.fullmatch(r"[A-Za-z_]\w*", args[0]):
                    raise ValueError(f"{path}: unsupported invocation of {ts[i]}")
                for generated in all_defs[ts[i]]:
                    out[args[0] if generated.startswith("$") else generated].add(path)
                i = end + 1
            else:
                i += 1
    return out


def inventory(root=ROOT):
    owners = defaultdict(set)
    crates = [root / "crates" / name for name in ("perry-runtime", "perry-stdlib", "perry-ffi")]
    crates += sorted((root / "crates").glob("perry-ext-*"))
    count = 0
    for crate in crates:
        sources = {str(p.relative_to(root)): p.read_text() for p in sorted((crate / "src").rglob("*.rs"))}
        exports = source_exports(sources)
        count += len(exports)
        for symbol, paths in exports.items():
            owners[symbol].update(paths)
    # Only cross-crate collisions. Target-specific implementations within one
    # crate are not the extension-vs-stdlib link-order hazard (#10678).
    duplicates = {s: sorted(paths) for s, paths in sorted(owners.items())
                  if len({Path(p).parts[1] for p in paths}) > 1}
    return duplicates, count, len(crates)


def check(actual, baseline):
    errors = []
    for symbol in sorted(actual.keys() | baseline.keys()):
        if actual.get(symbol) != baseline.get(symbol):
            errors.append(f"{symbol}: inventoried {actual.get(symbol, [])}; recorded {baseline.get(symbol, [])}")
    return errors


def no_raise(baseline, previous):
    return [symbol for symbol, paths in baseline.items()
            if not set(paths) <= set(previous.get(symbol, []))]


def self_test():
    plain = '#[no_mangle] pub extern "C" fn js_collision() {}'
    macro = 'macro_rules! entry { ($name:ident) => { #[no_mangle] pub extern "C" fn $name() {} }; } entry!(js_collision);'
    for body in (plain, macro, '#[unsafe(export_name = "js_collision")] pub extern "C" fn other() {}'):
        exports = source_exports({"crates/a/src/lib.rs": plain, "crates/b/src/lib.rs": body})
        assert len(exports["js_collision"]) == 2, exports
    assert not source_exports({"crates/a/src/lib.rs": '#[cfg(test)] mod tests; #[cfg(test)] #[no_mangle] fn test_stub() {}', "crates/a/src/tests.rs": plain})
    assert cfg_without_tests(tokens('all(test, not(feature = "runtime-link"))')) is False
    assert cfg_without_tests(tokens('any(test, feature = "runtime-link")')) is None
    assert tokens('"a\\\n b" br#"quoted \" token"# \'\\u{7b}\'') == ['"a\\\n b"', 'br#"quoted \" token"#', "'\\u{7b}'"]
    assert not source_exports({"x": '/* /* nested */ #[no_mangle] fn bogus() {} */ const DOC: &str = "#[no_mangle] fn bogus() {}"; extern "C" { fn js_import(); }'})
    assert check({"s": ["a", "b"]}, {})
    assert not check({"s": ["a", "b"]}, {"s": ["a", "b"]})
    assert check({"s": ["a", "b", "c"]}, {"s": ["a", "b"]})
    assert check({}, {"s": ["a", "b"]})  # removed collision needs baseline pruning
    assert no_raise({"s": ["a", "b", "c"]}, {"s": ["a", "b"]})
    assert not no_raise({}, {"s": ["a", "b"]})
    try:
        source_exports({"x": 'macro_rules! entry { ($first:ident, $second:ident) => { #[no_mangle] pub extern "C" fn $second() {} }; } entry!(a, b);'})
    except ValueError:
        pass
    else:
        raise AssertionError("unsupported export macro went unnoticed")
    try:
        source_exports({"x": macro + 'macro_rules! outer { ($name:ident) => { entry!($name); }; } outer!(js_collision);'})
    except ValueError:
        pass
    else:
        raise AssertionError("export wrapper macro went unnoticed")
    import tempfile
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        for crate, code in (("perry-runtime", plain), ("perry-ext-test", macro)):
            target = root / "crates" / crate / "src" / "lib.rs"
            target.parent.mkdir(parents=True)
            target.write_text(code)
        duplicates, count, _ = inventory(root)
        assert count == 2 and list(duplicates) == ["js_collision"]
        assert check(duplicates, {})
        (root / "crates/perry-ext-test/src/lib.rs").write_text('extern "C" { fn js_collision(); }')
        clean, _, _ = inventory(root)
        assert not clean and not check(clean, {})
    print("native export self-test: literal/renamed/macro collisions fail; comments/imports ignored; new providers and baseline increases fail")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--inventory", action="store_true", help="print the current duplicate inventory")
    parser.add_argument("--no-raise-vs", metavar="REF")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    actual, count, crates = inventory()
    if count < 500 or crates < 3:
        raise ValueError("native export scan compared too little; run from a complete repository")
    if args.inventory:
        print(json.dumps(actual, indent=2))
        return 0
    baseline = json.loads((ROOT / BASELINE).read_text())["duplicates"]
    errors = check(actual, baseline)
    if args.no_raise_vs:
        result = subprocess.run(["git", "show", f"{args.no_raise_vs}:{BASELINE}"], cwd=ROOT, capture_output=True, text=True)
        if result.returncode:
            # First introduction still must not add collisions beyond the
            # baseline ref's actual sources; never grant a blanket exception.
            import tempfile
            with tempfile.TemporaryDirectory() as tmp:
                archive = subprocess.run(["git", "archive", args.no_raise_vs, "crates"], cwd=ROOT, capture_output=True, check=True)
                subprocess.run(["tar", "-xf", "-", "-C", tmp], input=archive.stdout, check=True)
                previous, previous_count, previous_crates = inventory(Path(tmp))
                if previous_count < 500 or previous_crates < 3:
                    raise ValueError("base ref does not contain a complete native export inventory")
        else:
            previous = json.loads(result.stdout)["duplicates"]
        errors += [f"{s}: duplicate baseline increased vs {args.no_raise_vs}" for s in no_raise(baseline, previous)]
    print(f"native exports: {count} names in {crates} crates; {len(actual)} recorded cross-crate collisions")
    for error in errors:
        print(f"error: {error}", file=sys.stderr)
    if errors:
        return 1
    print("native export collision inventory unchanged; no new provider")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, AssertionError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        sys.exit(1)
