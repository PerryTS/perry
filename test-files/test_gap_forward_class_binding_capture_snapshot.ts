// A class declared in an async function or generator loop that names a
// forward-declared class (`Holder` reads `C`, which is declared after it and
// kept across a suspension) must not read `C` while it is still in its dead
// zone: Perry's own reads of the binding (a capture of it where `Holder` is
// declared) must not throw, while the program's own read of `C` before its
// declaration still does. Every line is compared with node. Each case runs on
// its own so that one failure does not hide the others (a case that throws
// prints `threw`).

async function attempt(label, fn) {
  try {
    console.log(label, JSON.stringify(await fn()));
  } catch (e) {
    console.log(label, 'threw', e && e.name);
  }
}

async function declaredClass() {
  const out = [];
  for (let i = 0; i < 2; i++) {
    const read = () => C1;
    class Holder1 {
      value() {
        return C1;
      }
    }
    class C1 extends Error {}
    await 0;
    out.push([new Holder1() instanceof Holder1, read() === C1]);
  }
  return out;
}

function* generatorClass() {
  for (let i = 0; i < 2; i++) {
    const read = () => C2;
    class Holder2 {
      value() {
        return C2;
      }
    }
    class C2 extends Error {}
    yield 0;
    yield [new Holder2() instanceof Holder2, read() === C2];
  }
}

async function classExpression() {
  const out = [];
  for (let i = 0; i < 2; i++) {
    const read = () => C3;
    const Holder3 = class Named3 {
      value() {
        return C3;
      }
    };
    class C3 extends Error {}
    await 0;
    out.push([new Holder3() instanceof Holder3, read() === C3]);
  }
  return out;
}

// `new Holder3()` before `C3` is declared forwards `C3` into the instance's
// captures; that forward is Perry's own read as well.
async function instanceBeforeDeclaration() {
  const out = [];
  for (let i = 0; i < 2; i++) {
    const read = () => C4;
    class Holder4 {
      value() {
        return C4;
      }
    }
    const early = new Holder4();
    class C4 extends Error {}
    await 0;
    out.push([early instanceof Holder4, read() === C4]);
  }
  return out;
}

// The program's own read of `C4` before its declaration still throws.
async function programReadStillThrows() {
  const out = [];
  for (let i = 0; i < 2; i++) {
    const read = () => C5;
    class Holder5 {
      value() {
        return C5;
      }
    }
    let seen;
    try {
      seen = read();
    } catch (e) {
      seen = e.name;
    }
    class C5 extends Error {}
    await 0;
    out.push([seen, new Holder5() instanceof Holder5, read() === C5]);
  }
  return out;
}

// Outside a loop or suspension nothing is converted: the same shapes hold.
async function plainFunction() {
  const read = () => C6;
  class Holder6 {
    value() {
      return C6;
    }
  }
  class C6 extends Error {}
  await 0;
  return [new Holder6() instanceof Holder6, read() === C6];
}

async function main() {
  await attempt('declared-class', declaredClass);
  await attempt('generator-class', () => [...generatorClass()].filter((v) => v !== 0));
  await attempt('class-expression', classExpression);
  await attempt('instance-before-declaration', instanceBeforeDeclaration);
  await attempt('program-read-still-throws', programReadStillThrows);
  await attempt('plain-function', plainFunction);
}
main();
