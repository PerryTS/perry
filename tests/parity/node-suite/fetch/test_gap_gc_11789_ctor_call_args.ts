// parity-env: PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_SCHEDULE_SEED=11789 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=4 PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_VERIFY_EVACUATION=1
// #11789 sweep: constructors that read an options literal. `new Headers({k: v})`
// took the key's raw string pointer BEFORE evaluating the value, and
// `new Response(body, init)`, `new Blob(parts, opts)`, `new ReadableStream({...})`
// and `super(a, b)` lowered each field / argument into a bare register and
// converted it where it was lowered. Output must be byte-identical to node.

function churn(rounds: number): number {
  let n = 0;
  for (let r = 0; r < rounds; r++) {
    const a: any[] = new Array(16);
    for (let j = 0; j < 16; j++) a[j] = { j, r, s: "v" + j };
    n += a.length;
  }
  return n;
}
const big: any = 4294967301;

// new Headers({ key: value }): the value collects after the key is read.
{
  const mkv = (): string => {
    churn(1500);
    return "v" + String(big);
  };
  const h = new Headers({ "x-k": mkv() });
  console.log(h.get("x-k"));
}

// new Response(body, { status, statusText }): the body is converted before the
// init is evaluated, and the init's fields are held across each other.
{
  const txt = (): string => {
    churn(1500);
    return "ok" + String(big);
  };
  const body = (): string => "b" + String(big);
  const r = new Response(body(), { status: 201, statusText: txt() });
  console.log(r.status, r.statusText);
}

// new Blob(parts, { type }): the parts array is held across the type.
async function blobShape() {
  const ty = (): string => {
    churn(1500);
    return "text/" + "plain";
  };
  const part = (): string => "p" + String(big);
  const b = new Blob([part()], { type: ty() });
  console.log(b.size, b.type, await b.text());
}

// new ReadableStream({ start, pull }): each callback is a closure built by a
// call that collects.
function streamShape() {
  const mkS = (tag: string) => {
    churn(1500);
    return () => {
      console.log(tag);
    };
  };
  const s = new ReadableStream({ start: mkS("start"), pull: mkS("pull") });
  console.log(typeof s);
}

// super(label, n): the label is held across the second argument.
class Base {
  label: string;
  n: number;
  constructor(label: string, n: number) {
    this.label = label;
    this.n = n;
  }
}
class Sub extends Base {
  constructor() {
    super("lbl " + String(big), churn(1500));
  }
}

streamShape();
const sub = new Sub();
console.log(sub.label, sub.n);
blobShape().then(() => console.log("done"));
