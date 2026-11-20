// An async function whose awaits sit inside an object literal WITH a spread
// must still suspend at the await and return a pending promise immediately —
// like V8 — instead of blocking the frame on the microtask pump.
//
// `{ v: await p, ...src }` lowers to a synthetic IIFE (`__perry_obj_iife`);
// two transform gaps left such functions un-CPS-rewritten: the hoist pre-pass
// never descended into the IIFE, and — for NESTED async closures (the
// turbopack factory shape) — the collect scan's closure-stop meant a function
// whose ONLY awaits sat in the IIFE never entered the work set at all. The
// resulting mid-frame drain reordered every promise race against the object
// build (the Next.js Flight row swap: next-intl's provider wrapper is exactly
// this shape).
//
// Detector: a properly-suspending call lets "after-call" log BEFORE the
// resolver tick; a busy-waiting call pumps the queue inside itself, flipping
// the order.

function scenario(name: string, makeCall: (p: Promise<string>) => any) {
  return new Promise<void>((done) => {
    let r: (v: string) => void;
    const pending = new Promise<string>((res) => (r = res));
    const log: string[] = [];
    Promise.resolve().then(() => {
      log.push("tick1");
      r!("V");
    });
    const ret = makeCall(pending);
    log.push("after-call");
    Promise.resolve(ret).then((v) => {
      log.push("resolved " + JSON.stringify(v));
      console.log(name + ": " + log.join(" | "));
      done();
    });
  });
}

async function f1(p: Promise<string>) {
  return await p;
}

function jsx(t: string, props: Record<string, unknown>) {
  return { t, props };
}

const src = { x: 1 };

// Top-level async fn, spread after/before the await.
async function spreadAfter(p: Promise<string>) {
  return { v: await p, ...src };
}
async function spreadBefore(p: Promise<string>) {
  return { ...src, v: await p };
}

// The production shape: NESTED async fn (factory arrow), conditional awaits
// in an object argument with a rest-spread that shadows the fn name.
const reg: { m?: (o: any) => Promise<any> } = {};
((a: typeof reg) => {
  async function m({ formats, locale, p, ...m }: any) {
    return jsx("g", {
      formats: void 0 === formats ? await f1(p) : formats,
      locale: locale ?? (await f1(p)),
      ...m,
    });
  }
  a.m = m;
})(reg);

async function main() {
  await scenario("top-level-spread-after", (p) => spreadAfter(p));
  await scenario("top-level-spread-before", (p) => spreadBefore(p));
  await scenario("nested-factory-conditional-awaits", (p) =>
    reg.m!({ p, extra: 2 }),
  );
  console.log("DONE");
}

main();
