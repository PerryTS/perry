// Scope context objects in loops: a loop body's captured-and-mutated
// `let`/`const` bindings get a FRESH scope object per iteration, while a
// function-scoped `var` stays one binding for the whole call.
declare function gc(): void;
function collect() {
  if (typeof gc === "function") gc();
}

// for (let …): per-iteration body bindings mutated by their closures.
function forLet() {
  const fns: Array<() => string> = [];
  for (let i = 0; i < 5; i++) {
    let hits = i * 10;
    let tag = "t" + i;
    fns.push(() => { hits++; tag += "!"; return `${tag}:${hits}`; });
    hits += 1;
  }
  collect();
  return fns.map((f) => f() + "|" + f()).join(" ");
}
console.log(forLet());

// for…of with a const binding and a mutated body let.
function forOf() {
  const out: Array<() => number> = [];
  for (const item of [3, 1, 4, 1, 5]) {
    let acc = item;
    out.push(() => (acc *= 2));
    acc += 100;
  }
  collect();
  return out.map((f) => f()).join(",");
}
console.log(forOf());

// while / do-while bodies are activated once per iteration too.
function whileLoops() {
  const fns: Array<() => number> = [];
  let k = 0;
  while (k < 4) {
    let local = k;
    fns.push(() => ++local);
    k++;
  }
  do {
    let d = k * 100;
    fns.push(() => (d -= 1));
    k--;
  } while (k > 2);
  collect();
  return fns.map((f) => f()).join(",");
}
console.log(whileLoops());

// A `var` inside a loop body is function-scoped: one shared binding.
function varInLoop() {
  const fns: Array<() => number> = [];
  for (let i = 0; i < 3; i++) {
    var shared = i;
    fns.push(() => shared++);
  }
  collect();
  return fns.map((f) => f()).join(",") + " " + shared;
}
console.log(varInLoop());

// Nested loops: the inner body's bindings are fresh per inner iteration and
// the outer body's per outer iteration; closures see both.
function nestedLoops() {
  const fns: Array<() => string> = [];
  for (let o = 0; o < 3; o++) {
    let outerCount = 0;
    for (let i = 0; i < 2; i++) {
      let innerCount = o * 10 + i;
      fns.push(() => `${++outerCount}/${++innerCount}`);
    }
  }
  collect();
  return fns.map((f) => f()).join(" ");
}
console.log(nestedLoops());

// break / continue inside a body that allocates its scope object.
function breakContinue() {
  const fns: Array<() => number> = [];
  for (let i = 0; i < 10; i++) {
    let v = i;
    if (i % 2 === 0) continue;
    fns.push(() => (v += 1000));
    if (i > 6) break;
  }
  return fns.map((f) => f()).join(",");
}
console.log(breakContinue());

// Many iterations with collections in between: every iteration's object must
// survive through the closures alone.
function churn() {
  const fns: Array<() => number> = [];
  for (let i = 0; i < 2000; i++) {
    let n = i;
    let pad = { i };
    fns.push(() => { n += pad.i; pad = { i: n }; return n; });
    if (i % 250 === 0) collect();
  }
  collect();
  let sum = 0;
  for (const f of fns) sum += f();
  return sum;
}
console.log(churn());
