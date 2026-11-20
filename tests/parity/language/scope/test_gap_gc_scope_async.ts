// Scope context objects through the async-to-generator and generator
// transforms: an async activation's locals live across awaits in the
// activation's scope object(s), user closures created inside keep only the
// bindings they name, and generator locals survive yields.
declare function gc(): void;
function collect() {
  if (typeof gc === "function") gc();
}
const tick = () => new Promise<void>((r) => setTimeout(r, 0));

async function locals(seed: number) {
  let a = seed;
  let text = "x";
  const list: number[] = [];
  for (let i = 0; i < 3; i++) {
    await tick();
    collect();
    a += i;
    text += i;
    list.push(a);
  }
  const reader = () => `${a}:${text}:${list.join("-")}`;
  await Promise.resolve();
  a *= 2;
  return reader;
}

async function closuresAcrossAwait() {
  let shared = 0;
  const inc = () => ++shared;
  inc();
  await tick();
  collect();
  inc();
  const late = () => shared * 10;
  await Promise.resolve();
  inc();
  return `${shared} ${late()}`;
}

// Per-iteration bindings inside an async loop whose body awaits before the
// declaration (the canonical `const data = await read(); cbs.push(() => data)`).
async function asyncLoopBindings() {
  const cbs: Array<() => string> = [];
  for (let i = 0; i < 4; i++) {
    const data = await Promise.resolve("d" + i);
    let uses = 0;
    cbs.push(() => `${data}#${++uses}`);
  }
  collect();
  return cbs.map((f) => f() + f()).join(" ");
}

// Several concurrent activations of one async function must not share cells.
async function concurrent(id: number) {
  let total = id;
  for (let k = 0; k < 3; k++) {
    await tick();
    total += id;
  }
  return total;
}

function* gen(limit: number) {
  let produced = 0;
  let last = "";
  const describe = () => `${produced}:${last}`;
  while (produced < limit) {
    last = "v" + produced;
    produced++;
    const sent: number | undefined = yield describe();
    if (sent) produced += sent;
    collect();
  }
  return describe();
}

async function main() {
  const r1 = await locals(1);
  const r2 = await locals(10);
  collect();
  console.log(r1(), r2());
  console.log(await closuresAcrossAwait());
  console.log(await asyncLoopBindings());
  console.log((await Promise.all([concurrent(1), concurrent(2), concurrent(3)])).join(","));
  const g = gen(5);
  const seen: string[] = [];
  let step = g.next();
  while (!step.done) {
    seen.push(String(step.value));
    step = g.next(seen.length === 2 ? 1 : 0);
  }
  seen.push("ret=" + step.value);
  console.log(seen.join(" "));
}
main();
