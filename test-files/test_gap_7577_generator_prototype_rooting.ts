// #7577: `js_generator_attach_prototype` held the generator instance's address
// across its own allocations. A copying minor in that window meant the
// `[[Prototype]]` link was recorded against the pre-move address and the
// pre-move address was RETURNED to the caller as the generator object.
//
// Without the GC instruments that is not a crash — it is a silently wrong
// prototype chain and a dangling handle, so this test pins the observable half:
// `Object.getPrototypeOf(gen())` and its identity, over enough constructions
// and enough allocation churn that the window is crossed thousands of times.

function* small(n: number): Generator<number, void, undefined> {
  for (let k = 0; k < n; k++) yield k + n;
}

async function* asmall(n: number): AsyncGenerator<number, void, undefined> {
  for (let k = 0; k < n; k++) yield k + n;
}

function churn(i: number): string {
  return "pad-" + i + "-" + (i * 7919) + "-" + (i % 13);
}

// --- the chain, on a single instance ---
const g0 = small(2);
const p0 = Object.getPrototypeOf(g0);
console.log("A proto is object:", typeof p0 === "object" && p0 !== null);
console.log("A proto === g.prototype:", p0 === small.prototype);
console.log("A proto !== instance:", (p0 as unknown) !== (g0 as unknown));

// `g.prototype` identity is stable across reads.
console.log("B stable:", small.prototype === small.prototype);

// Every instance of the same generator function shares one `g.prototype`.
console.log("C shared:", Object.getPrototypeOf(small(1)) === Object.getPrototypeOf(small(1)));

// --- the same, under construction + allocation churn ---
// Every iteration builds a generator, drains it, and allocates. The chain must
// still resolve on every instance, and the accumulated sum must be right — a
// returned-from-space generator would produce neither reliably.
const live: string[] = [];
let acc = 0;
let chainOk = 0;
for (let i = 0; i < 4000; i++) {
  const g = small(4);
  let gs = g.next();
  while (!gs.done) {
    acc += gs.value as number;
    gs = g.next();
  }
  if (Object.getPrototypeOf(g) === small.prototype) chainOk++;
  live.push(churn(i));
  if (live.length > 64) live.shift();
}
console.log("D acc:", acc);
console.log("D chainOk:", chainOk);
console.log("D live:", live.length);

// --- async generators take the queue-wrapper path on the same call ---
async function main(): Promise<void> {
  let asum = 0;
  let achainOk = 0;
  for (let i = 0; i < 200; i++) {
    const ag = asmall(3);
    for await (const v of ag) asum += v;
    if (Object.getPrototypeOf(asmall(1)) === asmall.prototype) achainOk++;
    churn(i);
  }
  console.log("E asum:", asum);
  console.log("E achainOk:", achainOk);
}

await main();
