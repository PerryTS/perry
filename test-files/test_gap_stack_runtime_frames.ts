// Error.stack must name the JS frames that sit BENEATH runtime frames: a
// callback the runtime invokes (map, forEach, sort, replace, the JSON.parse
// reviver, a Promise job, EventEmitter.emit, a getter or toJSON the runtime
// calls) and the JS function that called into the runtime. The capture steps
// runtime frames by their unwind tables on x86_64 Linux, where the runtime
// keeps no frame-pointer chain; this file is the witness that it does.
//
// Only the frame NAMES and their ORDER are compared with node: positions are
// `(<anonymous>)` in perry by design, and node's own internal frames
// (`Array.map`, `EventEmitter.emit`) are not ours to print. Every function
// here is prefixed `sw_` and only those frames are kept. Each case is
// entered through `run` with the function as a VALUE, so neither perry's
// inliner nor LLVM's can fold a `sw_` frame into its caller: inlined frames
// are a separate gap from the one this file witnesses.
import { EventEmitter } from "events";

function names(stack: string | undefined): string {
  const out: string[] = [];
  for (const line of (stack ?? "").split("\n").slice(1)) {
    const m = /^\s*at (?:async )?(?:new )?([^ (]+)/.exec(line);
    if (!m) continue;
    let name = m[1];
    const dot = name.lastIndexOf(".");
    if (dot >= 0) name = name.slice(dot + 1);
    if (name.startsWith("sw_")) out.push(name);
  }
  return out.join(" > ");
}

const results: string[] = [];
function report(label: string, stack: string | undefined): void {
  results.push(label + ": " + names(stack));
}

function sw_direct_inner(n: number): string | undefined {
  return n > 0 ? new Error("direct " + n).stack : undefined;
}
function sw_direct_outer(inner: (n: number) => string | undefined): string | undefined {
  const s = inner(1);
  return s ?? inner(2);
}

function sw_map_caller(xs: number[]): string | undefined {
  let captured: string | undefined;
  xs.map(function sw_map_cb(x: number) {
    if (captured === undefined) captured = new Error("map " + x).stack;
    return x * 2;
  });
  return captured;
}

function sw_foreach_caller(xs: number[]): string | undefined {
  let captured: string | undefined;
  xs.forEach(function sw_foreach_cb(x: number) {
    if (captured === undefined) captured = new Error("forEach " + x).stack;
  });
  return captured;
}

function sw_sort_caller(xs: number[]): string | undefined {
  let captured: string | undefined;
  xs.slice().sort(function sw_sort_cmp(a: number, b: number) {
    if (captured === undefined) captured = new Error("sort").stack;
    return a - b;
  });
  return captured;
}

function sw_replace_caller(s: string): string | undefined {
  let captured: string | undefined;
  s.replace(/b/g, function sw_replace_cb(m: string) {
    if (captured === undefined) captured = new Error("replace " + m).stack;
    return m.toUpperCase();
  });
  return captured;
}

function sw_reviver_caller(text: string): string | undefined {
  let captured: string | undefined;
  JSON.parse(text, function sw_reviver(k: string, v: unknown) {
    if (captured === undefined) captured = new Error("reviver " + k).stack;
    return v;
  });
  return captured;
}

function sw_emit_caller(e: EventEmitter): string | undefined {
  let captured: string | undefined;
  e.on("ev", function sw_listener(x: number) {
    if (captured === undefined) captured = new Error("emit " + x).stack;
  });
  e.emit("ev", 1);
  return captured;
}

function sw_getter_caller(): string | undefined {
  let captured: string | undefined;
  const o: Record<string, unknown> = {};
  Object.defineProperty(o, "g", {
    enumerable: true,
    get: function sw_getter() {
      if (captured === undefined) captured = new Error("getter").stack;
      return 1;
    },
  });
  Object.assign({}, o);
  return captured;
}

function sw_tojson_caller(): string | undefined {
  let captured: string | undefined;
  const o = {
    toJSON: function sw_tojson() {
      if (captured === undefined) captured = new Error("toJSON").stack;
      return 1;
    },
  };
  JSON.stringify({ o });
  return captured;
}

function sw_nested_caller(xs: number[][]): string | undefined {
  let captured: string | undefined;
  xs.map(function sw_nested_outer(row: number[]) {
    return row.map(function sw_nested_inner(x: number) {
      if (captured === undefined) captured = new Error("nested " + x).stack;
      return x;
    });
  });
  return captured;
}

function sw_thrown_caller(xs: number[]): string | undefined {
  try {
    xs.forEach(function sw_thrower(x: number) {
      if (x === 2) throw new Error("thrown " + x);
    });
  } catch (e) {
    return (e as Error).stack;
  }
  return undefined;
}

function run<A>(label: string, f: (a: A) => string | undefined, a: A): void {
  report(label, f(a));
}

function drive(): void {
  run("direct", sw_direct_outer, sw_direct_inner);
  run("map", sw_map_caller, [1, 2, 3]);
  run("forEach", sw_foreach_caller, [1, 2, 3]);
  run("sort", sw_sort_caller, [3, 1, 2]);
  run("replace", sw_replace_caller, "abcb");
  run("reviver", sw_reviver_caller, '{"a":1}');
  run("emit", sw_emit_caller, new EventEmitter());
  run("getter", sw_getter_caller, undefined);
  run("toJSON", sw_tojson_caller, undefined);
  run("nested", sw_nested_caller, [[1, 2], [3]]);
  run("thrown", sw_thrown_caller, [1, 2, 3]);
}
drive();

Promise.resolve(7).then(function sw_then(v: number) {
  report("promise", new Error("then " + v).stack);
  for (const r of results) console.log(r);
});
