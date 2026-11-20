// In an async function or generator, a lexical binding that a closure reads
// before its declaration is backed by a per-iteration cell (the state machine
// hands each loop iteration its own). The cell must stay in the dead zone until
// the declaration runs: a read before it raises the ReferenceError, as in node,
// and a read after it sees the initialized value. This holds for `let`, `const`
// and `class` bindings, and for closures kept across iterations. Every line is
// compared with node. Each case runs on its own so that one failure does not
// hide the others (a case that throws prints `threw`).

async function attempt(label, fn) {
  try {
    console.log(label, JSON.stringify(await fn()));
  } catch (e) {
    console.log(label, 'threw', e && e.name);
  }
}

async function letInAsyncLoop() {
  const out = [];
  for (let i = 0; i < 2; i++) {
    let seen;
    try {
      const read = () => x;
      seen = read();
    } catch (e) {
      seen = e.name;
    }
    let x = i + 1;
    await 0;
    out.push(seen + ':' + x);
  }
  return out;
}

async function constInAsyncLoop() {
  const out = [];
  for (let i = 0; i < 2; i++) {
    let seen;
    try {
      const read = () => x;
      seen = read();
    } catch (e) {
      seen = e.name;
    }
    const x = i + 10;
    await 0;
    out.push(seen + ':' + x);
  }
  return out;
}

function* letInGeneratorLoop() {
  for (let i = 0; i < 2; i++) {
    let seen;
    try {
      const read = () => x;
      seen = read();
    } catch (e) {
      seen = e.name;
    }
    let x = i + 1;
    yield seen + ':' + x;
  }
}

async function classInAsyncLoop() {
  const out = [];
  for (let i = 0; i < 2; i++) {
    let seen;
    try {
      const read = () => C;
      seen = typeof read();
    } catch (e) {
      seen = e.name;
    }
    class C extends Error {}
    await 0;
    out.push(seen + ':' + typeof C);
  }
  return out;
}

// Closures kept across iterations and awaits read their own iteration's value.
async function keptClosures() {
  const fns = [];
  for (let i = 0; i < 3; i++) {
    fns.push(() => x);
    const x = i * 10;
    await 0;
  }
  return fns.map((f) => f());
}

async function main() {
  await attempt('let-async', letInAsyncLoop);
  await attempt('const-async', constInAsyncLoop);
  await attempt('let-generator', () => [...letInGeneratorLoop()]);
  await attempt('class-async', classInAsyncLoop);
  await attempt('kept-closures', keptClosures);
}
main();
