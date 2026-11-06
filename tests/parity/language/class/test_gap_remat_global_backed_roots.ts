// String-literal locals and class-keys caches held across allocation-heavy
// code. Under the native root lowering both are rematerialized from their
// GC-root globals (`<mod>_.str.N.handle`, `@perry_class_keys_*`) instead of
// being relocated. A stale copy of either would print a wrong or empty
// literal here, or build instances over a forwarded keys array.

class Point {
  x: number;
  y: number;
  label: string;
  constructor(x: number, y: number, label: string) {
    this.x = x;
    this.y = y;
    this.label = label;
  }
}

class Tagged {
  kind: string;
  n: number;
  constructor(kind: string, n: number) {
    this.kind = kind;
    this.n = n;
  }
}

function churn(n: number): number {
  let acc = 0;
  for (let i = 0; i < n; i++) {
    const o = { a: i, b: "s" + i, c: [i, i + 1] };
    acc = (acc + o.b.length + o.c.length) | 0;
  }
  return acc;
}

function run(round: number): string {
  var greeting = "hello-remat-literal";
  var sep = "::";
  const tail = "the-end";
  let mode = "idle";
  const pts: Point[] = [];
  const tags: Tagged[] = [];
  let sum = 0;
  // Out-of-loop `new` sites take the outlined allocator, which reads the
  // function's class-keys cache (the slot this test rematerializes).
  const head = new Point(-1, -2, greeting);
  sum = (sum + churn(1500)) | 0;
  const first = new Tagged(tail, -1);
  for (let i = 0; i < 3000; i++) {
    pts.push(new Point(i, i * 2, greeting));
    if (i % 7 === 0) tags.push(new Tagged(tail, i));
    if (i % 500 === 0) {
      sum = (sum + churn(2000)) | 0;
      mode = "busy";
    }
  }
  let lens = 0;
  for (let i = 0; i < pts.length; i++) {
    lens += pts[i].label.length + pts[i].x - pts[i].y / 2;
  }
  let kinds = 0;
  for (let i = 0; i < tags.length; i++) {
    if (tags[i].kind === tail) kinds++;
  }
  sum = (sum + churn(1500)) | 0;
  const last = new Point(head.x, first.n, greeting);
  const keys = Object.keys(pts[round]).join(",") + "/" + Object.keys(last).join(",") + "/" + Object.keys(first).join(",");
  return greeting + sep + round + sep + lens + sep + sum + sep + kinds + sep + mode + sep + keys + sep + tail;
}

// Called once, before anything else collects: the literals are still in the
// nursery when `churn` runs its first copying minors, so they MOVE between the
// store into `banner` and the reads below it. Not in a loop and not a hot-loop
// callee, so its `new` sites take the outlined allocator and read the
// function's class-keys cache.
function setup(): string {
  var banner = "setup-banner-literal";
  const a = new Tagged(banner, 1);
  churn(30000);
  const b = new Point(3, 4, banner);
  churn(30000);
  const c = new Tagged("setup-tail", 2);
  return banner + "|" + a.kind + "|" + b.label + "|" + c.kind + "|" + Object.keys(b).join(",") + "|" + Object.keys(c).join(",");
}
console.log(setup());

const captured = "captured-literal";
const readers: Array<() => string> = [];
for (let r = 0; r < 20; r++) {
  const out = run(r);
  readers.push(() => captured + "#" + r);
  if (r % 5 === 0 || r === 19) console.log(out);
}
churn(20000);
console.log(readers.map((f) => f()).slice(0, 3).join(" "));
