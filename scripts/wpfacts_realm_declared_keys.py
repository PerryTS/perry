#!/usr/bin/env python3
"""The keys the runtime realm declares on the objects wpfacts rule R1 covers.

Rule R1 (perry-wpfacts builtins/undeclared.rs) makes a store under a key a
built-in object does not declare a program field of that object. That is
sound only if the solver's description (builtins/members.rs) knows every key
the runtime's realm declares there. This script reads the realm the runtime
build generates (OUT_DIR/realm-v1.json of perry-runtime or perry-codegen) and
writes the declared keys the solver must know to
crates/perry-wpfacts/src/builtins/realm_declared_keys.tsv; the wpfacts test
`builtins::realm_declared::the_solver_knows_every_key_the_realm_declares`
asserts them against members.rs. `--check` fails when the checked-in list is
not the one this realm gives (a runtime change that declares a new key).

Rows: `ns <Name> <key>` own string keys of a global constructor or namespace
object, and of a function a node module object holds (`node:http.ServerResponse`:
`prototype`; `node:stream.Readable`: `from`): a key the solver does not describe
there reads an unknown value, never an absent one; `proto
<Function.prototype|Object.prototype> <key>` their own string keys (the chain of every native function value and namespace); `fn <name>
<key>` own string keys other than length/name of a native prototype method.
`global %Global% <key>` own string keys of the global object: the globals
the realm defines (every other global name is the program's).
Symbol keys are left out of key rows: R1 never takes a symbol key.
`strings <owner> <key> <declaration>` rows name a key whose value the realm declares a host array of
strings (`value = dynamic, elements = string`): the array the runtime makes, its elements strings.
`accessor <owner> <key> <get> [<set>]` rows name a key the realm declares an
accessor: its getter (`-` none) and setter (no column: none). A [[Set]] of a key
whose accessor has no setter writes nothing (sloppy) or throws (strict).
`object <owner> <key> <namespace>` rows name a node module object's data property whose value
is another declared host object (`node:inspector` `default`: `node:inspector.default`;
`node:perf_hooks` `performance`: `performance`).
native <owner> <key> <identity>` rows resolve callable property references on global, namespace, node module (`node:fs`) and prototype holders
(an intrinsic prototype such as `%GeneratorPrototype%` included, its own keys only; aliases included) once, at generation, as canonical native function identities.
`held <Name> <key>` rows: own string keys other than length/name (every function's chain describes those) of a function
a host object that is no node module holds (`Temporal.Duration`: `prototype`, `from`, `compare`; `Intl.NumberFormat`:
`prototype`, `supportedLocalesOf`; `WebAssembly.Module`: `prototype`, `exports`): the same statement as an `ns` row of a
node module's function, in a kind of its own so that wpfacts can read the list without them (`reach::held_keys`).
"""
import argparse
import json
import pathlib
import sys

OUT = pathlib.Path(__file__).resolve().parent.parent / "crates/perry-wpfacts/src/builtins/realm_declared_keys.tsv"
FN_KINDS = {"method", "function", "getter", "setter", "alias"}


def namespace_holder(name, kind):
    """A global constructor or namespace object, or an intrinsic constructor
    named by its intrinsic (`%TypedArray%`, the [[Prototype]] of each typed
    array constructor): wpfacts reads their statics as a namespace's."""
    intrinsic_ctor = (kind == "constructor" and name.startswith("%") and name.endswith("%")
                      and "." not in name)
    return intrinsic_ctor or (kind in ("constructor", "namespace")
                              and not any(c in name for c in ".%:"))


def member_properties(declarations, holder, inherited):
    """Ordinary descriptor lookup: an own key shadows the same inherited key."""
    seen_keys, seen_holders = set(), set()
    while holder and holder["name"] not in seen_holders:
        seen_holders.add(holder["name"])
        for prop in holder.get("properties", []):
            key = (bool(prop.get("symbol")), prop["key"])
            if key not in seen_keys:
                seen_keys.add(key)
                yield prop
        holder = declarations.get(holder.get("prototype")) if inherited else None


def node_module(name, kind):
    """A node module object (`node:fs`), or an object one holds under a data
    property (`node:inspector.default`, `node:fs.promises`): wpfacts names it
    `BuiltinNs::Module`; its callable members resolve here (`native` rows), its
    keys are not R1's."""
    return kind == "namespace" and name.startswith("node:")


def host_object(name, kind):
    """A declared host object: a realm namespace (`node:fs`, `Intl`, `performance`);
    wpfacts reads it as `BuiltinNs::Module`."""
    return kind == "namespace"


def intrinsic_prototype(name, kind):
    """A realm prototype named by its intrinsic (`%GeneratorPrototype%`), not
    reachable as a constructor's `.prototype`: its own callable members only."""
    return kind == "prototype" and name.startswith("%") and name.endswith("%")


def rows(realm):
    out = set()
    declarations = {d["name"]: d for d in realm["declarations"]}
    receiver_result = [{"returns": {"slot": {"root": "this", "steps": []}}}]
    callable_names = {d["name"] for d in realm["declarations"]
                      if d["kind"] in FN_KINDS | {"constructor"}}
    module_values = {p["value"]["value"] for d in realm["declarations"] if node_module(d["name"], d["kind"])
                     for p in member_properties(declarations, d, True)
                     if p.get("value", {}).get("kind") == "reference" and p["value"]["value"] in callable_names}
    held_values = {p["value"]["value"] for d in realm["declarations"]
                   if host_object(d["name"], d["kind"]) and not node_module(d["name"], d["kind"])
                   for p in member_properties(declarations, d, True)
                   if p.get("value", {}).get("kind") == "reference" and p["value"]["value"] in callable_names}
    for d in realm["declarations"]:
        name, kind = d["name"], d["kind"]
        keys = [p["key"] for p in d.get("properties", []) if not p.get("symbol")]
        if (name.endswith(".prototype") or namespace_holder(name, kind) or name == "%Global%"
                or intrinsic_prototype(name, kind) or node_module(name, kind)):
            inherited = namespace_holder(name, kind) or name == "%Global%" or node_module(name, kind)
            for prop in member_properties(declarations, d, inherited):
                getter = prop.get("get")
                if getter and declarations[getter].get("effects") == receiver_result:
                    out.add(("getter-this", name, prop["key"], getter))
                if not prop.get("symbol") and (getter or prop.get("set")):
                    out.add(("accessor", name, prop["key"], getter or "-")
                            + ((prop["set"],) if prop.get("set") else ()))
                value = prop.get("value", {}).get("value")
                if prop.get("value", {}).get("kind") == "reference" and value in callable_names:
                    out.add(("native", name, prop["key"], value))
                if (node_module(name, kind) and prop.get("value", {}).get("kind") == "reference"
                        and host_object(value, declarations.get(value, {}).get("kind"))):
                    out.add(("object", name, prop["key"], value))
                if (prop.get("value", {}).get("kind") == "dynamic"
                        and declarations.get(value, {}).get("elements") == "string"):
                    out.add(("strings", name, prop["key"], value))
        if name == "%Global%":
            out.update(("global", name, k) for k in keys)
        if name in ("Function.prototype", "Object.prototype"):
            out.update(("proto", name, k) for k in keys)
        elif namespace_holder(name, kind):
            out.update(("ns", name, k) for k in keys)
        elif kind in FN_KINDS and ".prototype." in name:
            out.update(("fn", name, k) for k in keys if k not in ("length", "name"))
        elif name in module_values:
            out.update(("ns", name, k) for k in keys)
        elif name in held_values:
            out.update(("held", name, k) for k in keys if k not in ("length", "name"))
    return sorted(out)


def render(realm):
    head = "# generated by scripts/wpfacts_realm_declared_keys.py from realm-v1.json; do not edit\n"
    return head + "".join("\t".join(r) + "\n" for r in rows(realm))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("realm", help="realm-v1.json from a runtime build's OUT_DIR")
    ap.add_argument("--check", action="store_true")
    a = ap.parse_args()
    text = render(json.loads(pathlib.Path(a.realm).read_text()))
    if a.check:
        if OUT.read_text() != text:
            sys.exit(f"{OUT} is stale: rerun scripts/wpfacts_realm_declared_keys.py {a.realm}")
        print("realm declared keys: up to date")
    else:
        OUT.write_text(text)
        print(f"wrote {OUT} ({text.count(chr(10)) - 1} rows)")


if __name__ == "__main__":
    main()
