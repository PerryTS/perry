// Temporaries of a plain async activation that live and die inside one resume
// state need no heap-frame slot; everything that crosses a suspend, a catch or
// a closure still must. Output is compared against node.
const log: string[] = [];
function p<T>(v: T): Promise<T> { return Promise.resolve(v); }
function bad(msg: string): Promise<number> { return Promise.reject(new Error(msg)); }
function boom(msg: string): Promise<number> { throw new Error(msg); }

async function accumulate(n: number): Promise<number> {
  let acc = 0;
  for (let i = 0; i < n; i++) {
    acc += await p(i * 2);
    acc += (await p(i)) * 3 + (await p(1));
  }
  return acc;
}

async function nested(x: number): Promise<number> {
  return (await p((await p(x)) + 1)) * (await p(2));
}

async function crossSuspend(): Promise<string> {
  const a = p(10);              // operand temp used after another await
  const b = await p(5);
  const c = await a;
  const d = b + c;              // d crosses the next await
  await p(0);
  return `d=${d}`;
}

async function catches(): Promise<string> {
  const out: string[] = [];
  try {
    const t = await bad("rejected");
    out.push("no" + t);
  } catch (e) {
    out.push("caught:" + (e as Error).message);
  } finally {
    out.push("fin1");
  }
  try {
    const t = await boom("sync-throw-in-operand");
    out.push("no" + t);
  } catch (e) {
    out.push("caught:" + (e as Error).message);
  }
  let k = 0;
  for (let i = 0; i < 3; i++) {
    try {
      k += await (i === 1 ? bad("mid") : p(i));
    } catch (e) {
      k += 100;
    }
  }
  out.push("k=" + k);
  return out.join(",");
}

async function closures(): Promise<string> {
  const fns: Array<() => number> = [];
  for (let i = 0; i < 3; i++) {
    const v = await p(i * 10);
    fns.push(() => v + i);
  }
  const w = await p(7);
  const g = () => w * 2;
  return fns.map((f) => f()).join("|") + ":" + g();
}

async function conditional(flag: boolean): Promise<number> {
  const r = flag ? await p(1) : await p(2);
  const s = (flag && (await p(3))) || (await p(4));
  return r * 10 + (s as number);
}

async function main() {
  log.push("acc=" + (await accumulate(5)));
  log.push("nested=" + (await nested(4)));
  log.push(await crossSuspend());
  log.push(await catches());
  log.push(await closures());
  log.push("cond=" + (await conditional(true)) + "," + (await conditional(false)));
  const all = await Promise.all([accumulate(3), nested(1), conditional(true)]);
  log.push("all=" + all.join(";"));
  console.log(log.join("\n"));
}
main();
