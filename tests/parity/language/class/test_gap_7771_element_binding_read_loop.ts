// #7771: element-binding read loops (const r = a[i]; s += r.x + r.y) must
// behave exactly like node whether the element-shape clone's guard admits
// (h7: clean grown array), declines at runtime (h1 hole / h2 undefined /
// h4 Array subclass / h6 shrunk length), or the matcher declines statically
// (h8 mid-loop length write, h9 let-binding, h10 escaping binding).
// The different-shape-element case lives in #7776; the proxy case is
// test_gap_7771_proxy_array_read_loop.ts (separate file: #7775).
// Every read loop below is written in the
// EXACT body shape the widened matcher admits (`const r = a[i]; s += ...`),
// so the clone is emitted and the runtime guard / side exit — not the
// matcher — must reach correct behaviour. Byte-compared against node.
class P { x: number; y: number; constructor(x: number, y: number) { this.x = x; this.y = y; } }
class Q { x: string; y: string; constructor(x: string, y: string) { this.x = x; this.y = y; } }
class MyArr extends Array<P> {}

function build(n: number): P[] {
  const a: P[] = [];
  for (let i = 0; i < n; i++) a.push(new P(i, i + 1));
  return a;
}

// h1: hole punched by delete — guard must decline, slow clone throws like node
function h1(): number {
  const a = build(10);
  delete (a as any)[3];
  let s = 0;
  for (let i = 0; i < a.length; i++) { const r = a[i]; s += r.x + r.y; }
  return s;
}

// h2: element overwritten with undefined
function h2(): number {
  const a = build(10);
  (a as any)[5] = undefined;
  let s = 0;
  for (let i = 0; i < a.length; i++) { const r = a[i]; s += r.x + r.y; }
  return s;
}

// h4: Array subclass behind an Array-typed binding (#7573/#7603 brand shape)
function h4(): number {
  const a: P[] = new MyArr();
  for (let i = 0; i < 10; i++) a.push(new P(i, i + 1));
  let s = 0;
  for (let i = 0; i < a.length; i++) { const r = a[i]; s += r.x + r.y; }
  return s;
}

// h6: length reduced between build and read
function h6(): number {
  const a = build(10);
  a.length = 5;
  let s = 0;
  for (let i = 0; i < a.length; i++) { const r = a[i]; s += r.x + r.y; }
  return s;
}

// h7: growth far past the initial inline capacity, then read (the base shape)
function h7(): number {
  const a = build(100000);
  let s = 0;
  for (let i = 0; i < a.length; i++) { const r = a[i]; s += r.x + r.y; }
  return s;
}

// h8: length reduced MID-LOOP (3-statement body — matcher must decline; the
// generic loop re-reads the live length every iteration like node does)
function h8(): number {
  const a = build(10);
  let s = 0;
  for (let i = 0; i < a.length; i++) { const r = a[i]; s += r.x + r.y; if (i === 0) a.length = 3; }
  return s;
}

// h9: `let` binding instead of `const` (matcher declines; behaviour unchanged)
function h9(): number {
  const a = build(10);
  let s = 0;
  for (let i = 0; i < a.length; i++) { let r = a[i]; s += r.x + r.y; }
  return s;
}

// h10: binding escapes as a bare value (walk declines; behaviour unchanged)
let leak: P | null = null;
function h10(): number {
  const a = build(10);
  let s = 0;
  for (let i = 0; i < a.length; i++) { const r = a[i]; s += r.x + r.y + ((leak = r), 0); }
  return s;
}

function tryRun(tag: string, fn: () => number): void {
  try {
    console.log(tag, fn());
  } catch (e) {
    console.log(tag, "threw:", (e as Error).message);
  }
}
tryRun("h1", h1);
tryRun("h2", h2);
tryRun("h4", h4);
tryRun("h6", h6);
tryRun("h7", h7);
tryRun("h8", h8);
tryRun("h9", h9);
tryRun("h10", h10);
console.log("leak", leak ? (leak as P).x : -1);
