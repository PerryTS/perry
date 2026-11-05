// A closure that reads a class declared after it must read that binding, not
// the class's shared template, wherever in the closure the read sits. That
// includes the initializer of a `using` / `await using` declaration.
// (`for (using x of ...)` is a compile error in Perry, so it has no case
// here; the scanner's unit test covers that head.) Each case builds two scopes and
// checks that each closure read its own scope's class. Every line is compared
// with node. Each case runs on its own so that one failure does not hide the
// others (a case that throws prints `threw`).

function outcome(label, value) {
  console.log(label, typeof value === 'string' ? value : JSON.stringify(value));
}

async function check(label, make) {
  try {
    const a = make('a');
    const b = make('b');
    outcome(label, [(await a.read()) === a.C, (await b.read()) === b.C]);
  } catch (e) {
    outcome(label, 'threw ' + (e && e.name));
  }
}

async function main() {
  await check('using', function make1(tag) {
    const read = () => {
      const seen = [];
      using empty = (seen.push(C1), null);
      return seen[0];
    };
    class C1 extends Error {
      tag() {
        return tag;
      }
    }
    return { read, C: C1 };
  });

  await check('using-declarator-list', function make2(tag) {
    const read = () => {
      const seen = [];
      using first = null, second = (seen.push(C2), null);
      return seen[0];
    };
    class C2 extends Error {
      tag() {
        return tag;
      }
    }
    return { read, C: C2 };
  });

  await check('await-using', function make3(tag) {
    const read = async () => {
      const seen = [];
      await using empty = (seen.push(C3), null);
      return seen[0];
    };
    class C3 extends Error {
      tag() {
        return tag;
      }
    }
    return { read, C: C3 };
  });

  await check('using-in-block', function make5(tag) {
    const read = () => {
      const seen = [];
      {
        using empty = (seen.push(C5), null);
      }
      return seen[0];
    };
    class C5 extends Error {
      tag() {
        return tag;
      }
    }
    return { read, C: C5 };
  });

  // The default class shape: nothing captured, so the class is a shared
  // template and the read names it directly.
  await check('using-plain-class', function make6(tag) {
    const read = () => {
      const seen = [];
      using empty = (seen.push(C6), null);
      return seen[0];
    };
    class C6 {}
    return { read, C: C6 };
  });
}
main();
