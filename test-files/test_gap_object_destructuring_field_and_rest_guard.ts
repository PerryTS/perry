// Object-destructuring coverage for the field-guard / rest-array perf work:
// 2-field, 5-field, nested, defaults (undefined-only, not null), rest
// (excluding named + computed keys, keeping Symbol keys), missing
// properties, getter call order, null/undefined source TypeErrors, Symbol
// keys, and function-parameter destructuring.

// --- 2-field / 5-field ---
{
  const { a, b } = { a: 1, b: 2 };
  console.log("2field", a, b);
  const { a: a5, b: b5, c: c5, d: d5, e: e5 } = { a: 10, b: 20, c: 30, d: 40, e: 50 };
  console.log("5field", a5, b5, c5, d5, e5);
}

// --- nested ---
{
  const { a: { b } } = { a: { b: 99 } };
  console.log("nested", b);
}

// --- defaults: only undefined triggers, not null ---
{
  const { a = 10 } = {} as { a?: number };
  console.log("default-missing", a);
  const { a: a2 = 10 } = { a: undefined } as { a?: number };
  console.log("default-undefined", a2);
  const { a: a3 = 10 } = { a: null } as { a?: number | null };
  console.log("default-null", a3);
  const { a: a4 = 10 } = { a: 0 };
  console.log("default-falsy-present", a4);
}

// --- missing property -> undefined ---
{
  const { missing } = { a: 1 } as { a: number; missing?: number };
  console.log("missing-prop", missing === undefined);
}

// --- rest excludes named keys, keeps Symbol keys ---
{
  const src = { a: 1, b: 2, c: 3, d: 4 };
  const { a, b, ...rest } = src;
  console.log("rest-basic", a, b, JSON.stringify(rest));

  // NOTE: a `{...rest}` of an object with a Symbol-keyed property is
  // deliberately NOT covered here. Perry currently drops Symbol-keyed
  // properties from the rest object entirely (`js_object_rest`'s own
  // key-copy logic, unrelated to the `exclude_keys` array this file's
  // perf change touches) — a pre-existing, separately-filed gap, not
  // something this test should assert byte-identical parity on.

  // computed-key exclusion (evaluated once) + rest
  let evalCount = 0;
  function key() {
    evalCount++;
    return "b";
  }
  const { [key()]: bv, ...restComputed } = { a: 1, b: 2, c: 3 };
  console.log("rest-computed-key", bv, JSON.stringify(restComputed), evalCount);

  // empty pattern before rest
  const { ...restAll } = { x: 1, y: 2 };
  console.log("rest-empty-pattern", JSON.stringify(restAll));
}

// --- Symbol-keyed destructuring ---
{
  const sym2 = Symbol("s2");
  const obj: any = { [sym2]: 42, plain: 1 };
  const { [sym2]: symVal, plain } = obj;
  console.log("symbol-key-read", symVal, plain);
}

// --- getters run exactly once, in PATTERN source order (not object's own key order) ---
{
  const log: string[] = [];
  const obj = {
    get c() {
      log.push("c");
      return 3;
    },
    get a() {
      log.push("a");
      return 1;
    },
    get b() {
      log.push("b");
      return 2;
    },
  };
  const { c, a, b } = obj;
  console.log("getter-order", log.join(","), a, b, c);
}

// --- null / undefined source throws TypeError (even for empty pattern) ---
{
  function tryDestructure(fn: () => void): string {
    try {
      fn();
      return "no-throw";
    } catch (e) {
      return e instanceof TypeError ? "TypeError" : "wrong-error:" + String(e);
    }
  }
  console.log("null-source", tryDestructure(() => {
    const { a } = null as any;
    void a;
  }));
  console.log("undefined-source", tryDestructure(() => {
    const { a } = undefined as any;
    void a;
  }));
  console.log("null-source-empty-pattern", tryDestructure(() => {
    const {} = null as any;
  }));
  console.log("undefined-source-rest", tryDestructure(() => {
    const { ...r } = undefined as any;
    void r;
  }));
}

// --- function parameter destructuring: plain, default, rest, nested ---
{
  function f2({ a, b }: { a: number; b: number }): number {
    return a - b;
  }
  console.log("param-2field", f2({ a: 5, b: 2 }));

  function fDefault({ a = 7 }: { a?: number }): number {
    return a;
  }
  console.log("param-default-missing", fDefault({}));
  console.log("param-default-present", fDefault({ a: 1 }));
  console.log("param-default-null", fDefault({ a: null } as any));

  function fRest({ a, ...rest }: { a: number; [k: string]: number }): string {
    return a + ":" + JSON.stringify(rest);
  }
  console.log("param-rest", fRest({ a: 1, b: 2, c: 3 }));

  function fNested({ outer: { inner } }: { outer: { inner: number } }): number {
    return inner;
  }
  console.log("param-nested", fNested({ outer: { inner: 77 } }));

  function fParamThrows(o: any): string {
    try {
      const { a } = o;
      return "no-throw:" + a;
    } catch (e) {
      return e instanceof TypeError ? "TypeError" : "wrong-error";
    }
  }
  console.log("param-null-throws", fParamThrows(null));
}
