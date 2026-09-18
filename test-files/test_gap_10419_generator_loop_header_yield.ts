// #10419: a `yield` in a loop CONDITION / UPDATE / INIT must suspend the
// generator at every nesting depth. #5933 fixed loops that are direct
// statements of the generator body; the same loop nested in `if` / `else` /
// `try` / `catch` / `finally` / `switch` / a label / another loop was emitted
// as an ordinary loop, so the residual yield never suspended: `[...g()]` was
// empty and a sent-value loop never terminated. Minifiers produce exactly this
// shape (lru-cache 11.5.2's `*#A` / `*#z` iterators).
//
// Every loop body calls tick(), which throws after 1000 steps, and every
// consumer caps its pulls, so a regression prints a wrong line instead of
// hanging the harness.

let steps = 0;
function tick(): void {
  if (++steps > 1000) throw new Error("runaway loop");
}

function take(it: Iterator<any>, sends: any[] = [], limit = 25): string {
  const out: string[] = [];
  let r = it.next();
  let i = 0;
  while (!r.done && i < limit) {
    out.push(JSON.stringify(r.value));
    r = it.next(sends[i]);
    i++;
  }
  out.push(r.done ? "done=" + JSON.stringify(r.value) : "(pull limit)");
  return out.join(" ");
}

function run(name: string, mk: () => Iterator<any>, sends?: any[]): void {
  steps = 0;
  try {
    console.log(name + ":", take(mk(), sends));
  } catch (e) {
    console.log(name + ": threw", (e as Error).message);
  }
}

function attempt(name: string, f: () => string): void {
  steps = 0;
  try {
    console.log(name + ":", f());
  } catch (e) {
    console.log(name + ": threw", String(e instanceof Error ? e.message : e));
  }
}

// ── issue repro ──────────────────────────────────────────────────────────
function* noIf() {
  for (let t = 2; t >= 0 && ((yield t), t !== 0); ) t = t - 1;
}
function* forInIf(n: number) {
  if (n) for (let t = 2; t >= 0 && ((yield t), t !== 0); ) t = t - 1;
}
function* whileInIf(n: number) {
  let t = 2;
  if (n) while (((yield t), t > 0)) t = t - 1;
}
function* forInTry() {
  try { for (let t = 2; ((yield t), t > 0); ) t = t - 1; } finally {}
}
console.log(JSON.stringify([...noIf()]));
console.log(JSON.stringify([...forInIf(1)]));
console.log(JSON.stringify([...whileInIf(1)]));
console.log(JSON.stringify([...forInTry()]));

// ── loop kind x header position x container (each yields 3 2 1) ──────────
function* m_while_cond_top(n: number) { let t = 3; while (((yield t), --t > 0)) tick(); }
function* m_while_cond_if(n: number) { if (n) { let t = 3; while (((yield t), --t > 0)) tick(); } }
function* m_while_cond_else(n: number) { if (!n) tick(); else { let t = 3; while (((yield t), --t > 0)) tick(); } }
function* m_while_cond_try(n: number) { try { let t = 3; while (((yield t), --t > 0)) tick(); } finally { tick(); } }
function* m_while_cond_catch(n: number) { try { throw new Error("x"); } catch { let t = 3; while (((yield t), --t > 0)) tick(); } }
function* m_while_cond_finally(n: number) { try { tick(); } finally { let t = 3; while (((yield t), --t > 0)) tick(); } }
function* m_while_cond_switch(n: number) { switch (n) { case 0: break; case 1: { let t = 3; while (((yield t), --t > 0)) tick(); } break; default: tick(); } }
function* m_while_cond_label(n: number) { blk: { let t = 3; while (((yield t), --t > 0)) tick(); if (n) break blk; tick(); } }
function* m_while_cond_labeled_loop(n: number) { let t = 3; lbl: while (((yield t), --t > 0)) { tick(); continue lbl; } }
function* m_while_cond_if_labeled_loop(n: number) { if (n) { let t = 3; lbl: while (((yield t), --t > 0)) { tick(); continue lbl; } } }
function* m_while_cond_loop(n: number) { for (let i = 0; i < n; i++) { let t = 3; while (((yield t), --t > 0)) tick(); } }
function* m_while_cond_deep(n: number) { if (n) try { switch (n) { case 1: { let t = 3; while (((yield t), --t > 0)) tick(); } } } catch (e) { throw e; } }
function* m_dowhile_cond_top(n: number) { let t = 3; do tick(); while (((yield t), --t > 0)); }
function* m_dowhile_cond_if(n: number) { if (n) { let t = 3; do tick(); while (((yield t), --t > 0)); } }
function* m_dowhile_cond_else(n: number) { if (!n) tick(); else { let t = 3; do tick(); while (((yield t), --t > 0)); } }
function* m_dowhile_cond_try(n: number) { try { let t = 3; do tick(); while (((yield t), --t > 0)); } finally { tick(); } }
function* m_dowhile_cond_catch(n: number) { try { throw new Error("x"); } catch { let t = 3; do tick(); while (((yield t), --t > 0)); } }
function* m_dowhile_cond_finally(n: number) { try { tick(); } finally { let t = 3; do tick(); while (((yield t), --t > 0)); } }
function* m_dowhile_cond_switch(n: number) { switch (n) { case 0: break; case 1: { let t = 3; do tick(); while (((yield t), --t > 0)); } break; default: tick(); } }
function* m_dowhile_cond_label(n: number) { blk: { let t = 3; do tick(); while (((yield t), --t > 0)); if (n) break blk; tick(); } }
function* m_dowhile_cond_labeled_loop(n: number) { let t = 3; lbl: do { tick(); continue lbl; } while (((yield t), --t > 0)); }
function* m_dowhile_cond_if_labeled_loop(n: number) { if (n) { let t = 3; lbl: do { tick(); continue lbl; } while (((yield t), --t > 0)); } }
function* m_dowhile_cond_loop(n: number) { for (let i = 0; i < n; i++) { let t = 3; do tick(); while (((yield t), --t > 0)); } }
function* m_dowhile_cond_deep(n: number) { if (n) try { switch (n) { case 1: { let t = 3; do tick(); while (((yield t), --t > 0)); } } } catch (e) { throw e; } }
function* m_for_cond_top(n: number) { for (let t = 3; ((yield t), t > 1); t--) tick(); }
function* m_for_cond_if(n: number) { if (n) for (let t = 3; ((yield t), t > 1); t--) tick(); }
function* m_for_cond_else(n: number) { if (!n) tick(); else for (let t = 3; ((yield t), t > 1); t--) tick(); }
function* m_for_cond_try(n: number) { try { for (let t = 3; ((yield t), t > 1); t--) tick(); } finally { tick(); } }
function* m_for_cond_catch(n: number) { try { throw new Error("x"); } catch { for (let t = 3; ((yield t), t > 1); t--) tick(); } }
function* m_for_cond_finally(n: number) { try { tick(); } finally { for (let t = 3; ((yield t), t > 1); t--) tick(); } }
function* m_for_cond_switch(n: number) { switch (n) { case 0: break; case 1: { for (let t = 3; ((yield t), t > 1); t--) tick(); } break; default: tick(); } }
function* m_for_cond_label(n: number) { blk: { for (let t = 3; ((yield t), t > 1); t--) tick(); if (n) break blk; tick(); } }
function* m_for_cond_labeled_loop(n: number) { lbl: for (let t = 3; ((yield t), t > 1); t--) { tick(); continue lbl; } }
function* m_for_cond_if_labeled_loop(n: number) { if (n) { lbl: for (let t = 3; ((yield t), t > 1); t--) { tick(); continue lbl; } } }
function* m_for_cond_loop(n: number) { for (let i = 0; i < n; i++) { for (let t = 3; ((yield t), t > 1); t--) tick(); } }
function* m_for_cond_deep(n: number) { if (n) try { switch (n) { case 1: { for (let t = 3; ((yield t), t > 1); t--) tick(); } } } catch (e) { throw e; } }
function* m_for_update_top(n: number) { for (let t = 3; t > 0; (yield t), t--) tick(); }
function* m_for_update_if(n: number) { if (n) for (let t = 3; t > 0; (yield t), t--) tick(); }
function* m_for_update_else(n: number) { if (!n) tick(); else for (let t = 3; t > 0; (yield t), t--) tick(); }
function* m_for_update_try(n: number) { try { for (let t = 3; t > 0; (yield t), t--) tick(); } finally { tick(); } }
function* m_for_update_catch(n: number) { try { throw new Error("x"); } catch { for (let t = 3; t > 0; (yield t), t--) tick(); } }
function* m_for_update_finally(n: number) { try { tick(); } finally { for (let t = 3; t > 0; (yield t), t--) tick(); } }
function* m_for_update_switch(n: number) { switch (n) { case 0: break; case 1: { for (let t = 3; t > 0; (yield t), t--) tick(); } break; default: tick(); } }
function* m_for_update_label(n: number) { blk: { for (let t = 3; t > 0; (yield t), t--) tick(); if (n) break blk; tick(); } }
function* m_for_update_labeled_loop(n: number) { lbl: for (let t = 3; t > 0; (yield t), t--) { tick(); continue lbl; } }
function* m_for_update_if_labeled_loop(n: number) { if (n) { lbl: for (let t = 3; t > 0; (yield t), t--) { tick(); continue lbl; } } }
function* m_for_update_loop(n: number) { for (let i = 0; i < n; i++) { for (let t = 3; t > 0; (yield t), t--) tick(); } }
function* m_for_update_deep(n: number) { if (n) try { switch (n) { case 1: { for (let t = 3; t > 0; (yield t), t--) tick(); } } } catch (e) { throw e; } }
function* m_for_init_top(n: number) { let t = 0; for (yield 3; t < 2; t++) yield 2 - t; }
function* m_for_init_if(n: number) { if (n) { let t = 0; for (yield 3; t < 2; t++) yield 2 - t; } }
function* m_for_init_else(n: number) { if (!n) tick(); else { let t = 0; for (yield 3; t < 2; t++) yield 2 - t; } }
function* m_for_init_try(n: number) { try { let t = 0; for (yield 3; t < 2; t++) yield 2 - t; } finally { tick(); } }
function* m_for_init_catch(n: number) { try { throw new Error("x"); } catch { let t = 0; for (yield 3; t < 2; t++) yield 2 - t; } }
function* m_for_init_finally(n: number) { try { tick(); } finally { let t = 0; for (yield 3; t < 2; t++) yield 2 - t; } }
function* m_for_init_switch(n: number) { switch (n) { case 0: break; case 1: { let t = 0; for (yield 3; t < 2; t++) yield 2 - t; } break; default: tick(); } }
function* m_for_init_label(n: number) { blk: { let t = 0; for (yield 3; t < 2; t++) yield 2 - t; if (n) break blk; tick(); } }
function* m_for_init_labeled_loop(n: number) { let t = 0; lbl: for (yield 3; t < 2; t++) { yield 2 - t; continue lbl; } }
function* m_for_init_if_labeled_loop(n: number) { if (n) { let t = 0; lbl: for (yield 3; t < 2; t++) { yield 2 - t; continue lbl; } } }
function* m_for_init_loop(n: number) { for (let i = 0; i < n; i++) { let t = 0; for (yield 3; t < 2; t++) yield 2 - t; } }
function* m_for_init_deep(n: number) { if (n) try { switch (n) { case 1: { let t = 0; for (yield 3; t < 2; t++) yield 2 - t; } } } catch (e) { throw e; } }

const matrix: Array<[string, (n: number) => Iterator<any>]> = [
  ["while_cond_top", m_while_cond_top],
  ["while_cond_if", m_while_cond_if],
  ["while_cond_else", m_while_cond_else],
  ["while_cond_try", m_while_cond_try],
  ["while_cond_catch", m_while_cond_catch],
  ["while_cond_finally", m_while_cond_finally],
  ["while_cond_switch", m_while_cond_switch],
  ["while_cond_label", m_while_cond_label],
  ["while_cond_labeled_loop", m_while_cond_labeled_loop],
  ["while_cond_if_labeled_loop", m_while_cond_if_labeled_loop],
  ["while_cond_loop", m_while_cond_loop],
  ["while_cond_deep", m_while_cond_deep],
  ["dowhile_cond_top", m_dowhile_cond_top],
  ["dowhile_cond_if", m_dowhile_cond_if],
  ["dowhile_cond_else", m_dowhile_cond_else],
  ["dowhile_cond_try", m_dowhile_cond_try],
  ["dowhile_cond_catch", m_dowhile_cond_catch],
  ["dowhile_cond_finally", m_dowhile_cond_finally],
  ["dowhile_cond_switch", m_dowhile_cond_switch],
  ["dowhile_cond_label", m_dowhile_cond_label],
  ["dowhile_cond_labeled_loop", m_dowhile_cond_labeled_loop],
  ["dowhile_cond_if_labeled_loop", m_dowhile_cond_if_labeled_loop],
  ["dowhile_cond_loop", m_dowhile_cond_loop],
  ["dowhile_cond_deep", m_dowhile_cond_deep],
  ["for_cond_top", m_for_cond_top],
  ["for_cond_if", m_for_cond_if],
  ["for_cond_else", m_for_cond_else],
  ["for_cond_try", m_for_cond_try],
  ["for_cond_catch", m_for_cond_catch],
  ["for_cond_finally", m_for_cond_finally],
  ["for_cond_switch", m_for_cond_switch],
  ["for_cond_label", m_for_cond_label],
  ["for_cond_labeled_loop", m_for_cond_labeled_loop],
  ["for_cond_if_labeled_loop", m_for_cond_if_labeled_loop],
  ["for_cond_loop", m_for_cond_loop],
  ["for_cond_deep", m_for_cond_deep],
  ["for_update_top", m_for_update_top],
  ["for_update_if", m_for_update_if],
  ["for_update_else", m_for_update_else],
  ["for_update_try", m_for_update_try],
  ["for_update_catch", m_for_update_catch],
  ["for_update_finally", m_for_update_finally],
  ["for_update_switch", m_for_update_switch],
  ["for_update_label", m_for_update_label],
  ["for_update_labeled_loop", m_for_update_labeled_loop],
  ["for_update_if_labeled_loop", m_for_update_if_labeled_loop],
  ["for_update_loop", m_for_update_loop],
  ["for_update_deep", m_for_update_deep],
  ["for_init_top", m_for_init_top],
  ["for_init_if", m_for_init_if],
  ["for_init_else", m_for_init_else],
  ["for_init_try", m_for_init_try],
  ["for_init_catch", m_for_init_catch],
  ["for_init_finally", m_for_init_finally],
  ["for_init_switch", m_for_init_switch],
  ["for_init_label", m_for_init_label],
  ["for_init_labeled_loop", m_for_init_labeled_loop],
  ["for_init_if_labeled_loop", m_for_init_if_labeled_loop],
  ["for_init_loop", m_for_init_loop],
  ["for_init_deep", m_for_init_deep],
];

for (const [name, g] of matrix) {
  run(name, () => g(1));
}
// the untaken branch / zero-iteration outer loop yields nothing
run("while_cond_if n=0", () => m_while_cond_if(0));
run("for_update_loop n=0", () => m_for_update_loop(0));
run("for_cond_loop n=2", () => m_for_cond_loop(2));

// ── minified lru-cache iterator shape ────────────────────────────────────
class MiniLRU {
  #n = 0;
  #h = 0;
  #a = 0;
  #u: number[] = [];
  #k: string[] = [];
  #stale: boolean[] = [];
  allowStale = false;
  set(k: string, stale = false): this {
    const i = this.#k.length;
    this.#k.push(k);
    this.#stale.push(stale);
    this.#u.push(i + 1);
    if (this.#n === 0) this.#h = i;
    this.#a = i;
    this.#n++;
    return this;
  }
  #V(t: number): boolean {
    return t < this.#k.length;
  }
  #p(t: number): boolean {
    return this.#stale[t];
  }
  *#A({ allowStale: e = this.allowStale } = {}) {
    if (this.#n) for (let t = this.#h; this.#V(t) && ((e || !this.#p(t)) && (yield t), t !== this.#a); ) t = this.#u[t];
  }
  *keys() {
    for (const i of this.#A()) yield this.#k[i];
  }
  *allKeys() {
    for (const i of this.#A({ allowStale: true })) yield this.#k[i];
  }
}
{
  const c = new MiniLRU().set("a").set("b", true).set("c");
  console.log("lru keys:", JSON.stringify([...c.keys()]), JSON.stringify([...c.allKeys()]));
  console.log("lru empty:", JSON.stringify([...new MiniLRU().keys()]));
}

// ── sent values terminate the loop ───────────────────────────────────────
function* sentBounded(n: number) {
  let count = 0;
  let v: any;
  if (n) {
    while ((v = yield count) !== "stop" && count < 5) count++;
  }
  return count;
}
run("sent bounded", () => sentBounded(1), [undefined, undefined, "stop"]);
run("sent bounded exhausts", () => sentBounded(1));
function* sentUnbounded(n: number) {
  let count = 0;
  let v: any;
  try {
    if (n) while ((v = yield count) !== "stop") { tick(); count++; }
  } finally {
    count += 100;
  }
  return count;
}
run("sent unbounded", () => sentUnbounded(1), [1, 2, 3, "stop"]);
function* sentDoWhile(n: number) {
  const got: any[] = [];
  let v: any;
  switch (n) {
    case 1:
      do { tick(); } while ((v = yield got.length) !== undefined && got.push(v) < 10);
  }
  return got;
}
run("sent do-while", () => sentDoWhile(1), ["x", "y"]);
function* sentForUpdate(n: number) {
  let total = 0;
  if (n) for (let i = 0; i < 10; i += (yield total) ?? 1) { tick(); total += i; }
  return total;
}
run("sent for-update", () => sentForUpdate(1), [4, 5]);
function* sentForInit(n: number) {
  if (n) for (let t = yield "init"; t > 0; t--) yield t;
}
run("sent for-init", () => sentForInit(1), [3]);
run("sent for-init none", () => sentForInit(1));

// ── return() / throw() while suspended in a header yield ──────────────────
function* retMid(log: string[], n: number) {
  if (n) {
    try {
      for (let t = 0; ((yield t), t < 100); t++) tick();
    } finally {
      log.push("finally");
    }
  }
  log.push("unreachable");
}
attempt("return mid", () => {
  const log: string[] = [];
  const g = retMid(log, 1);
  return JSON.stringify([g.next(), g.next(), g.return(42), g.next()]) + " " + log.join(",");
});
function* throwMid(n: number) {
  if (n) {
    try {
      while (((yield "w"), true)) tick();
    } catch (e) {
      yield "caught " + e;
    }
  }
  yield "after";
}
attempt("throw mid", () => {
  const g = throwMid(1);
  return JSON.stringify([g.next(), g.next(), g.throw("boom"), g.next(), g.next()]);
});
function* throwOuterCatch(n: number) {
  try {
    if (n) for (let i = 0; i < 100; (yield i), i++) tick();
  } catch (e) {
    return "outer " + e;
  }
}
attempt("throw outer", () => {
  const g = throwOuterCatch(1);
  return JSON.stringify([g.next(), g.next(), g.throw("bang"), g.next()]);
});

// ── break / continue around header yields ────────────────────────────────
function* breakInSwitch(n: number) {
  let t = 0;
  switch (n) {
    case 1:
      while (((yield t), true)) {
        tick();
        if (t++ >= 2) break;
      }
      yield "after";
  }
}
run("break in switch", () => breakInSwitch(1));
function* labeledContinueUpdate(n: number) {
  if (n) {
    outer: for (let i = 0; i < 3; (yield i), i++) {
      for (let j = 0; j < 2; j++) {
        tick();
        if (j === 1) continue outer;
      }
    }
  }
}
run("labeled continue update", () => labeledContinueUpdate(1));
function* continueTryFinallyUpdate(log: string[], n: number) {
  for (let i = 0; i < 3; (yield "u" + i), i++) {
    try {
      tick();
      if (i === 1) continue;
      log.push("b" + i);
    } finally {
      log.push("f" + i);
    }
  }
  if (n) {
    for (let i = 0; i < 2; (yield "v" + i), i++) {
      try {
        if (i === 0) continue;
      } finally {
        log.push("g" + i);
      }
    }
  }
}
{
  const log: string[] = [];
  run("continue try/finally update", () => continueTryFinallyUpdate(log, 1));
  console.log("  log:", log.join(","));
}
function* nestedHeaders(n: number) {
  if (n) for (let i = 0; ((yield "i" + i), i < 2); i++) while (((yield "j" + i), false)) tick();
}
run("nested headers", () => nestedHeaders(1));

function* closuresPerIteration(n: number) {
  const fns: Array<() => number> = [];
  if (n) for (let i = 0; ((yield i), i < 2); i++) fns.push(() => i);
  if (n) for (let i = 0; i < 2; (yield "u" + i), i++) fns.push(() => i * 10);
  return fns.map((f) => f()).join(",");
}
run("closures per iteration", () => closuresPerIteration(1));

// ── controls: shapes that already worked ─────────────────────────────────
function* bodyYieldInIf(n: number) {
  if (n) for (let t = 0; t < 3; t++) yield t;
}
run("control body yield in if", () => bodyYieldInIf(1));
function* forOfYieldIterable(n: number) {
  if (n) for (const x of (yield "want") as number[]) yield x * 2;
}
run("control for-of yield iterable", () => forOfYieldIterable(1), [[1, 2]]);
function* noYieldHeaderInIf(n: number) {
  let s = 0;
  if (n) for (let i = 0; i < 4; i++) s += i;
  yield s;
}
run("control no header yield", () => noYieldHeaderInIf(1));

// ── async generators (for await) ─────────────────────────────────────────
async function* aCondInIf(n: number) {
  if (n) for (let t = 3; ((yield t), t > 1); t--) tick();
}
async function* aWhileInTry(n: number) {
  let t = 3;
  try {
    while (((yield t), --t > 0)) tick();
  } finally {
    tick();
  }
}
async function* aDoWhileInSwitch(n: number) {
  let t = 3;
  switch (n) {
    case 1:
      do tick(); while (((yield t), --t > 0));
  }
}
async function* aUpdateInLabel(n: number) {
  if (n) {
    lbl: for (let t = 3; t > 0; (yield t), t--) {
      tick();
      continue lbl;
    }
  }
}
async function* aPromiseOperand(n: number) {
  for (let t = 3; ((yield Promise.resolve(t * 10)), t > 1); t--) tick();
  if (n) while (((yield Promise.resolve("p")), false)) tick();
}
async function* aAwaitAndYield(n: number) {
  let t = 2;
  if (n) while (((yield await Promise.resolve(t)), t-- > 0)) tick();
}
async function* aSent(n: number) {
  let v: any;
  let count = 0;
  if (n) {
    while ((v = yield count) !== "stop") {
      tick();
      count++;
    }
  }
  return count;
}
async function* aReturnMid(log: string[], n: number) {
  if (n) {
    try {
      for (let t = 0; ((yield t), t < 100); t++) tick();
    } finally {
      log.push("async finally");
    }
  }
}

// A plain async function's `for (let x = await p; …)` becomes a for-init yield
// after the await→yield rewrite, so it shares the init hoist.
async function forInitAwait(n: number): Promise<string> {
  const out: number[] = [];
  for (let x = await Promise.resolve(3); x > 0; x--) out.push(x);
  if (n) for (let y = await Promise.resolve(2); y > 0; y--) out.push(y * 10);
  return out.join(",");
}

async function collect(name: string, g: AsyncIterable<any>): Promise<void> {
  steps = 0;
  const out: string[] = [];
  try {
    for await (const v of g) {
      out.push(v instanceof Promise ? "<promise>" : JSON.stringify(v));
      if (out.length > 25) break;
    }
    console.log(name + ":", out.join(" "));
  } catch (e) {
    console.log(name + ": threw", (e as Error).message, out.join(" "));
  }
}

async function main(): Promise<void> {
  await collect("async cond in if", aCondInIf(1));
  await collect("async while in try", aWhileInTry(1));
  await collect("async do-while in switch", aDoWhileInSwitch(1));
  await collect("async update in label", aUpdateInLabel(1));
  await collect("async promise operand", aPromiseOperand(1));
  await collect("async await and yield", aAwaitAndYield(1));

  steps = 0;
  const s = aSent(1);
  const r: any[] = [];
  r.push(await s.next());
  r.push(await s.next("a"));
  r.push(await s.next("b"));
  r.push(await s.next("stop"));
  r.push(await s.next());
  console.log("async sent:", JSON.stringify(r));

  console.log("async fn for-init await:", await forInitAwait(1));

  const log: string[] = [];
  const g = aReturnMid(log, 1);
  const rr: any[] = [];
  rr.push(await g.next());
  rr.push(await g.next());
  rr.push(await g.return(7));
  rr.push(await g.next());
  console.log("async return mid:", JSON.stringify(rr), log.join(","));
}
main().catch((e) => console.log("async main threw", (e as Error).message));
