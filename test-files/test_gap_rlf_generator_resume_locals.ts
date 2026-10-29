// RLF: resume-local generator controls. Every line is compared with node.
// Covers return/throw injection mid-loop, try/finally inside generator loops,
// yield* delegation, nested generators, exceptions crossing frames, early
// return from for-of, and generators resumed across garbage collections.

const log: string[] = [];
function out(s: string): void {
  log.push(s);
}

// 1. return() injected mid-loop runs the pending finally exactly once.
function* loopFinally(n: number): Generator<number, string, number> {
  let acc = 0;
  for (let i = 0; i < n; i++) {
    try {
      acc += yield i;
    } finally {
      out("finally " + i);
    }
  }
  return "done " + acc;
}
{
  const g = loopFinally(5);
  out("a " + JSON.stringify(g.next(1)));
  out("b " + JSON.stringify(g.next(10)));
  out("c " + JSON.stringify(g.return("early")));
  out("d " + JSON.stringify(g.next(5)));
}

// 2. throw() injected mid-loop is caught inside the loop and the loop continues.
function* catching(): Generator<number, number, undefined> {
  let caught = 0;
  for (let i = 0; i < 6; i++) {
    try {
      yield i;
    } catch (e) {
      caught++;
      out("caught " + String(e) + " at " + i);
    }
  }
  return caught;
}
{
  const g = catching();
  out("e " + JSON.stringify(g.next()));
  out("f " + JSON.stringify(g.throw("x1")));
  out("g " + JSON.stringify(g.next()));
  out("h " + JSON.stringify(g.throw("x2")));
  let last = g.next();
  while (!last.done) last = g.next();
  out("i " + JSON.stringify(last));
}

// 3. An unhandled throw completes the generator; later resumes are done.
function* thrower(): Generator<number> {
  yield 1;
  yield 2;
  throw new Error("boom");
}
{
  const g = thrower();
  out("j " + JSON.stringify(g.next()));
  out("k " + JSON.stringify(g.next()));
  try {
    g.next();
  } catch (e) {
    out("l " + (e as Error).message);
  }
  out("m " + JSON.stringify(g.next()));
  out("n " + JSON.stringify(g.return(7)));
}

// 4. A throw from a callee frame unwinds through the generator.
function inner(n: number): number {
  if (n > 2) throw new RangeError("deep " + n);
  return n * 2;
}
function* viaCallee(): Generator<number> {
  for (let i = 0; i < 6; i++) yield inner(i);
}
{
  const seen: number[] = [];
  try {
    for (const v of viaCallee()) seen.push(v);
  } catch (e) {
    out("o " + (e as Error).message + " after " + seen.join(","));
  }
}

// 5. yield* delegation, with return values and injection into the delegate.
function* leaf(tag: string, n: number): Generator<string, number, string> {
  let got = "";
  try {
    for (let i = 0; i < n; i++) got += yield tag + i;
  } finally {
    out("leaf finally " + tag);
  }
  return got.length;
}
function* branch(): Generator<string, string, string> {
  const a = yield* leaf("a", 2);
  const b = yield* leaf("b", 3);
  return "r" + a + b;
}
{
  const g = branch();
  const r: string[] = [];
  let step = g.next("");
  let k = 0;
  while (!step.done) {
    r.push(step.value);
    step = g.next("xy".slice(0, k++ % 3));
  }
  out("p " + r.join(",") + " => " + step.value);
  const h = branch();
  h.next("");
  h.next("q");
  out("q " + JSON.stringify(h.return("cut")));
  const t = branch();
  t.next("");
  t.next("q");
  t.next("q");
  try {
    t.throw(new Error("into b"));
  } catch (e) {
    out("r " + (e as Error).message);
  }
  out("s " + JSON.stringify(t.next("")));
}

// 6. Nested generators: a generator that drives other generators.
function* nat(): Generator<number> {
  let i = 0;
  while (true) yield i++;
}
function* take(g: Generator<number>, n: number): Generator<number> {
  for (const v of g) {
    if (n-- <= 0) return;
    yield v;
  }
}
function* pairs(): Generator<number[]> {
  for (const a of take(nat(), 3)) {
    for (const b of take(nat(), 2)) {
      yield [a, b];
    }
  }
}
{
  const r: string[] = [];
  for (const p of pairs()) r.push(p.join(":"));
  out("t " + r.join(" "));
}

// 7. Early exit from for-of calls return(), which runs the finally.
function* closing(): Generator<number> {
  try {
    for (let i = 0; i < 10; i++) yield i;
  } finally {
    out("closing finally");
  }
}
{
  let sum = 0;
  for (const v of closing()) {
    if (v === 3) break;
    sum += v;
  }
  out("u " + sum);
  const lab = (() => {
    for (const v of closing()) {
      if (v === 2) return "ret" + v;
    }
    return "none";
  })();
  out("v " + lab);
}

// 8. try/finally with yield in finally and a rethrow.
function* finallyYield(): Generator<string, void, undefined> {
  try {
    yield "body";
    throw new Error("e1");
  } catch (e) {
    yield "catch " + (e as Error).message;
    throw new Error("e2");
  } finally {
    yield "finally";
  }
}
{
  const g = finallyYield();
  const r: string[] = [];
  try {
    for (let s = g.next(); !s.done; s = g.next()) r.push(s.value);
  } catch (e) {
    r.push("threw " + (e as Error).message);
  }
  out("w " + r.join("|"));
}

// 9. A running generator rejects re-entry.
let reenter: Generator<number> | null = null;
function* selfRef(): Generator<number> {
  yield 1;
  try {
    reenter!.next();
  } catch (e) {
    out("x " + (e instanceof TypeError));
  }
  yield 2;
}
{
  reenter = selfRef();
  out("y " + JSON.stringify(reenter.next()));
  out("z " + JSON.stringify(reenter.next()));
  out("z2 " + JSON.stringify(reenter.next()));
}

// 10. Generators resumed across garbage collections (moving collector).
function* churn(n: number): Generator<string> {
  const keep: string[] = [];
  const objs: { a: number; b: string }[] = [];
  for (let i = 0; i < n; i++) {
    objs.push({ a: i, b: "v" + i });
    keep.push("s" + i);
    yield keep[i] + "/" + objs[i].b + "/" + keep.length;
  }
  yield "total " + keep.length + " " + objs.length;
}
{
  const gens = [churn(40), churn(40), churn(40)];
  let h = 0;
  let live = 0;
  for (let round = 0; round < 41; round++) {
    for (const g of gens) {
      const junk: number[][] = [];
      for (let k = 0; k < 200; k++) junk.push([k, round, h]);
      const s = g.next();
      if (!s.done) {
        live++;
        for (let c = 0; c < s.value.length; c++) h = (h * 31 + s.value.charCodeAt(c)) | 0;
      }
      h = (h + junk.length) | 0;
    }
  }
  out("aa " + live + " " + h);
}

for (const line of log) console.log(line);
