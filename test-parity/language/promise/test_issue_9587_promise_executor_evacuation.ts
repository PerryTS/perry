// #9587 — a `new Promise(executor)` whose executor ALLOCATES must still return
// the live Promise.
//
// The executor is arbitrary user code and it runs before `new Promise` returns.
// Claude Code's dialog helper is the real-world shape:
//
//     new Promise((res) => { const z = (y) => void res(y); root.render(ui(z)) })
//
// — an entire ink/React render inside the executor. An evacuating young
// collection there MOVES the promise. The runtime kept the pre-collection
// address in a bare local and handed that back, so the `await` either fell
// through immediately on a recycled header that decoded as `Fulfilled`, or
// parked its continuation on the dead copy while `resolve()` settled the live
// one — a silent, permanent hang.
//
// Node prints:
//   order:created,resolving value:resolved
//   settled-before-resolve:false

let saved: (() => void) | null = null;
const order: string[] = [];

function dialog(): Promise<string> {
  return new Promise<string>((resolve) => {
    saved = () => resolve("resolved");
    // Stand in for the render: allocate hard enough to trip an evacuating minor
    // while this promise is young.
    let sink: any = null;
    for (let i = 0; i < 200000; i++) {
      sink = { a: i, b: [i, i + 1], c: "row" + i, d: { e: i } };
    }
    if (sink === null) console.log("unreachable");
  });
}

let settled = false;

async function main(): Promise<void> {
  const p = dialog();
  order.push("created");
  p.then(() => { settled = true; });
  setTimeout(() => {
    order.push("resolving");
    console.log("settled-before-resolve:" + settled);
    (saved as () => void)();
  }, 10);
  const v = await p;
  let vs = "unreadable";
  try {
    vs = (v as unknown) === "resolved" ? "resolved" : "other";
  } catch {
    vs = "threw";
  }
  order.push("value:" + vs);
  console.log("order:" + order.join(","));
}

main();
