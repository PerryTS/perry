// In an async function or generator, a lexical binding that a closure reads
// before its declaration is backed by a per-iteration cell. While the binding
// is in its dead zone every access to it throws a ReferenceError, not only a
// read: a plain assignment (after it evaluates its right-hand side), an
// update (`x++`, `--x`), a compound assignment and a logical assignment (those
// read first, so they throw before evaluating theirs), and a write from a
// closure. A failed write leaves the binding in its dead zone, so the
// declaration still initializes it and it behaves as usual afterwards. Every
// line is compared with node. Each case runs on its own so that one failure
// does not hide the others (a case that throws prints `threw`).

async function attempt(label, fn) {
  try {
    console.log(label, JSON.stringify(await fn()));
  } catch (e) {
    console.log(label, 'threw', e && e.name);
  }
}

// Each case runs its dead-zone access in two loop iterations (a fresh cell
// each); a `log` records which right-hand sides were evaluated.
async function assign() {
  const out = [];
  for (let i = 0; i < 2; i++) {
    const read = () => x;
    let seen = 'accepted';
    try {
      x = 9;
    } catch (e) {
      seen = e.name;
    }
    let x = 1;
    await 0;
    out.push([seen, read(), x]);
  }
  return out;
}

async function postfix() {
  const out = [];
  for (let i = 0; i < 2; i++) {
    const read = () => x;
    let seen = 'accepted';
    try {
      x++;
    } catch (e) {
      seen = e.name;
    }
    let x = 1;
    await 0;
    out.push([seen, read(), x]);
  }
  return out;
}

async function prefix() {
  const out = [];
  for (let i = 0; i < 2; i++) {
    const read = () => x;
    let seen = 'accepted';
    try {
      --x;
    } catch (e) {
      seen = e.name;
    }
    let x = 1;
    await 0;
    out.push([seen, read(), x]);
  }
  return out;
}

async function compound() {
  const out = [];
  for (let i = 0; i < 2; i++) {
    const log = [];
    const read = () => x;
    let seen = 'accepted';
    try {
      x += (log.push('rhs'), 1);
    } catch (e) {
      seen = e.name;
    }
    let x = 1;
    await 0;
    out.push([seen, log, read(), x]);
  }
  return out;
}

// A plain assignment evaluates its right-hand side before it throws.
async function assignOrder() {
  const out = [];
  for (let i = 0; i < 2; i++) {
    const log = [];
    const read = () => x;
    let seen = 'accepted';
    try {
      x = (log.push('rhs'), 9);
    } catch (e) {
      seen = e.name;
    }
    let x = 1;
    await 0;
    out.push([seen, log, read(), x]);
  }
  return out;
}

async function logical() {
  const out = [];
  for (let i = 0; i < 2; i++) {
    const log = [];
    const read = () => x;
    let seen = 'accepted';
    try {
      x ??= (log.push('rhs'), 5);
    } catch (e) {
      seen = e.name;
    }
    let x = 1;
    await 0;
    out.push([seen, log, read(), x]);
  }
  return out;
}

async function fromClosure() {
  const out = [];
  for (let i = 0; i < 2; i++) {
    const read = () => x;
    const write = () => {
      x = 9;
    };
    let seen = 'accepted';
    try {
      write();
    } catch (e) {
      seen = e.name;
    }
    let x = 1;
    await 0;
    out.push([seen, read(), x]);
  }
  return out;
}

function* inGenerator() {
  for (let i = 0; i < 2; i++) {
    const read = () => x;
    let seen = 'accepted';
    try {
      x++;
    } catch (e) {
      seen = e.name;
    }
    let x = 1;
    yield 0;
    yield [seen, read(), x];
  }
}

async function classBinding() {
  const out = [];
  for (let i = 0; i < 2; i++) {
    const read = () => K;
    let seen = 'accepted';
    try {
      K = 9;
    } catch (e) {
      seen = e.name;
    }
    class K {}
    await 0;
    out.push([seen, typeof read(), read() === K]);
  }
  return out;
}

// After the declaration, writes and updates behave as usual, from the loop
// body and from the closures alike.
async function afterDeclaration() {
  const out = [];
  for (let i = 0; i < 2; i++) {
    const read = () => x;
    const bump = () => {
      x = x + 100;
    };
    let x = i;
    await 0;
    x = 9;
    x++;
    x += 1;
    bump();
    out.push([read(), x]);
  }
  return out;
}

async function main() {
  await attempt('assign', assign);
  await attempt('postfix', postfix);
  await attempt('prefix', prefix);
  await attempt('compound', compound);
  await attempt('assign-order', assignOrder);
  await attempt('logical', logical);
  await attempt('from-closure', fromClosure);
  await attempt('generator', () => [...inGenerator()].filter((v) => v !== 0));
  await attempt('class-binding', classBinding);
  await attempt('after-declaration', afterDeclaration);
}
main();
