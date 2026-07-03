#!/usr/bin/env bash
source "$(dirname "$0")/_perry_test_lib.sh"

# A subclass of a builtin (Error/Map/Set/Array) with NO explicit constructor
# must still run its instance field initializers — they run right after the
# implicit super(). Regression: the synthesized default constructor dropped
# them (new E().tag was 0 instead of the initializer value). The explicit-ctor
# case (M2) already worked and guards against a regression the other way.
perry_run main.ts <<'TS'
class E1 extends Error { tag = "e1"; count = 1 + 2; }
class M1 extends Map { tag = "m1"; }
class S1 extends Set { tag = "s1"; }
class A1 extends Array { tag = "a1"; }
class M2 extends Map { tag = "m2"; constructor() { super(); } }

const e = new E1();
console.log(JSON.stringify({
  errTag: e.tag,
  errCount: (e as any).count,
  errIsError: e instanceof Error,
  errMessage: (() => { try { throw new E1("boom"); } catch (x: any) { return x.message; } })(),
  mapTag: new M1().tag,
  setTag: new S1().tag,
  arrTag: (new A1() as any).tag,
  explicitCtorTag: new M2().tag,
}));
TS

perry_expect_node
perry_pass "builtin subclass implicit-ctor field initializers"
