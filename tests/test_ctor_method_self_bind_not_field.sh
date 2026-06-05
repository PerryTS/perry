#!/usr/bin/env bash
set -euo pipefail

# A constructor `this.method = this.method.bind(this)` is a METHOD OVERRIDE, not a
# new data field. Previously the ctor-body field-inference pass added `method` as an
# inline field (it excluded declared/inherited fields + accessors, but NOT methods),
# so the codegen field branch shadowed the method: `this.method` read the
# uninitialised slot (`undefined`) BEFORE the assignment ran, and `.bind(this)` on it
# threw "Bind must be called on a function". zod's `ZodType` constructor self-binds
# ~20 methods this way, so every `z.string()` / `z.object()` / … threw at construction.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$REPO_ROOT/target/release/perry}}"
if [[ ! -x "$PERRY" ]]; then PERRY="$REPO_ROOT/target/debug/perry"; fi
if [[ ! -x "$PERRY" ]]; then
    echo "SKIP: perry binary not found (build with cargo build -p perry)"
    exit 0
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

cat >"$TMPDIR/m.ts" <<'TS'
// Mirrors zod's ZodType: self-bind a batch of methods in the constructor, with a
// subclass instantiated via a static factory.
abstract class ZodType<O = any> {
  _def: any;
  spa = (this as any).safeParseAsync;
  constructor(def: any) {
    this._def = def;
    this.parse = this.parse.bind(this);
    this.safeParse = this.safeParse.bind(this);
    this.parseAsync = this.parseAsync.bind(this);
    this.safeParseAsync = this.safeParseAsync.bind(this);
    (this as any).spa = (this as any).spa.bind(this);
    this.optional = this.optional.bind(this);
    this.default = this.default.bind(this);
    this.catch = this.catch.bind(this);
  }
  abstract _parse(x: any): any;
  parse(d: unknown): O { return this._parse(d); }
  safeParse(d: unknown) { return { success: true, data: this._parse(d) }; }
  async parseAsync(d: unknown): Promise<O> { return this._parse(d); }
  async safeParseAsync(d: unknown) { return { success: true, data: this._parse(d) }; }
  optional() { return "optional"; }
  default(v: any) { return v; }
  catch(v: any) { return v; }
}
class ZodString extends ZodType<string> {
  _parse(x: any) { return "parsed:" + x; }
  static create = (): ZodString => new ZodString({ t: "s" });
}

const s: any = ZodString.create();
if (typeof s.parse !== "function") throw new Error("parse not a function");
if (s.parse("hi") !== "parsed:hi") throw new Error("parse() => " + s.parse("hi"));
if (s.optional() !== "optional") throw new Error("optional() => " + s.optional());
if (s.safeParse("x").data !== "parsed:x") throw new Error("safeParse data wrong");
console.log("OK");
TS

OUT="$("$PERRY" run "$TMPDIR/m.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }
if ! grep -q "^OK$" <<<"$OUT"; then echo "FAIL: expected OK, got:"; echo "$OUT"; exit 1; fi
echo "PASS: ctor method self-bind is not a field"
