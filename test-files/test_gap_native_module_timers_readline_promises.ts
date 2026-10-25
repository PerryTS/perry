// req("timers/promises") and req("readline/promises"): native-module
// namespaces whose exports must run the real implementations, not answer
// undefined, however they are read or called.
import { createRequire } from "node:module";
const req = createRequire(import.meta.url);
"use strict";
const timers = req("timers");
const readline = req("readline");
const { PassThrough } = req("stream");
const tp = req("timers/promises");
const tpPrefixed = req("node:timers/promises");
const { setTimeout: sleep, setImmediate: immediate } = req("timers/promises");
const rlp = req("readline/promises");
const rlpPrefixed = req("node:readline/promises");
const { createInterface } = req("readline/promises");
const show = async (label, fn) => {
  try { console.log(label, JSON.stringify(await fn())); }
  catch (e) { console.log(label, "threw", e && e.name); }
};
(async () => {
  for (const n of ["setTimeout", "setImmediate", "setInterval"]) {
    console.log("timers/promises", n, typeof tp[n], typeof tpPrefixed[n], tp[n] === timers.promises[n]);
  }
  console.log("timers/promises scheduler", typeof tp.scheduler, typeof tp.scheduler.wait, typeof tp.scheduler.yield,
    tp.scheduler === timers.promises.scheduler);
  console.log("timers/promises namespace", tp === timers.promises, tpPrefixed === tp);
  await show("setTimeout member", () => tp.setTimeout(5, "member"));
  await show("setTimeout prefixed", () => tpPrefixed.setTimeout(5, "prefixed"));
  await show("setTimeout comma", () => (0, tp.setTimeout)(5, "comma"));
  await show("setTimeout destructured", () => sleep(5, "destructured"));
  await show("setImmediate member", () => tp.setImmediate("member"));
  await show("setImmediate destructured", () => immediate("destructured"));
  await show("setTimeout abort", async () => {
    const ac = new AbortController();
    const p = tp.setTimeout(1000, "late", { signal: ac.signal });
    ac.abort();
    try { await p; return "resolved"; } catch (e) { return e.name + " " + e.code; }
  });
  await show("setInterval", async () => {
    const seen = [];
    for await (const v of tp.setInterval(5, "tick")) { seen.push(v); if (seen.length === 2) break; }
    return seen;
  });
  await show("scheduler.wait", () => tp.scheduler.wait(5));
  await show("scheduler.yield", () => tp.scheduler.yield());

  for (const n of ["createInterface", "Interface", "Readline"]) {
    console.log("readline/promises", n, typeof rlp[n], typeof rlpPrefixed[n], rlp[n] === readline.promises[n]);
  }
  console.log("readline/promises namespace", rlp === readline.promises, rlpPrefixed === rlp);
  const ask = async (make) => {
    const input = new PassThrough();
    const output = new PassThrough();
    const rl = make({ input, output });
    const answer = rl.question("name? ");
    input.write("perry\n");
    const value = await answer;
    rl.close();
    return value;
  };
  await show("question member", () => ask((o) => rlp.createInterface(o)));
  await show("question prefixed", () => ask((o) => rlpPrefixed.createInterface(o)));
  await show("question comma", () => ask((o) => (0, rlp.createInterface)(o)));
  await show("question destructured", () => ask((o) => createInterface(o)));
  await show("question Interface", () => ask((o) => new rlp.Interface(o)));
  await show("Readline", async () => {
    const out = new PassThrough();
    const r = new rlp.Readline(out);
    r.cursorTo(0).clearLine(0);
    await r.commit();
    out.end();
    return out.read().toString();
  });
  process.exit(0);
})();
