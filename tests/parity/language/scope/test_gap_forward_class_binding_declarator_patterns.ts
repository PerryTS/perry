// A closure that reads a class declared after it must read that binding, not
// the class's shared template, wherever in the closure the read sits. That
// includes the executable parts of a binding pattern: the defaults and the
// computed keys of an object or array pattern in a `const` / `let`
// declaration, a `for-of` head or a `catch` parameter, as well as the
// defaults of a function parameter and of an array assignment pattern. Each case
// builds two scopes and checks that each closure read its own scope's class.
// Every line is compared with node. Each case runs on its own so that one
// failure does not hide the others (a case that throws prints `threw`).

function outcome(label, value) {
  console.log(label, typeof value === 'string' ? value : JSON.stringify(value));
}

function threw(e) {
  return 'threw ' + (e && e.name);
}

function check(label, make) {
  try {
    const a = make('a');
    const b = make('b');
    outcome(label, [a.read() === a.C, b.read() === b.C]);
  } catch (e) {
    outcome(label, threw(e));
  }
}

check('object-default', function make1(tag) {
  const read = () => {
    const { ctor = C1 } = {};
    return [tag, ctor][1];
  };
  class C1 extends Error {}
  return { read, C: C1 };
});

check('object-default-capturing', function make2(tag) {
  const read = () => {
    const { ctor = C2 } = {};
    return ctor;
  };
  class C2 {
    value() {
      return tag;
    }
  }
  return { read, C: C2 };
});

check('array-default', function make3(tag) {
  const read = () => {
    const [ctor = C3] = [];
    return ctor;
  };
  class C3 {
    value() {
      return tag;
    }
  }
  return { read, C: C3 };
});

check('nested-default', function make4(tag) {
  const read = () => {
    const {
      inner: { ctor = C4 },
    } = { inner: {} };
    return ctor;
  };
  class C4 {
    value() {
      return tag;
    }
  }
  return { read, C: C4 };
});

check('let-default', function make5(tag) {
  const read = () => {
    let { ctor = C5 } = {};
    return ctor;
  };
  class C5 {
    value() {
      return tag;
    }
  }
  return { read, C: C5 };
});

check('computed-key', function make6(tag) {
  const seen = [];
  const read = () => {
    const { [(seen.push(C6), 'k')]: v } = { k: 1 };
    return seen[seen.length - 1];
  };
  class C6 {
    value() {
      return tag;
    }
  }
  return { read, C: C6 };
});

check('for-of-default', function make7(tag) {
  const read = () => {
    let got;
    for (const { ctor = C7 } of [{}]) {
      got = ctor;
    }
    return got;
  };
  class C7 {
    value() {
      return tag;
    }
  }
  return { read, C: C7 };
});

check('catch-default', function make8(tag) {
  const read = () => {
    try {
      throw {};
    } catch ({ ctor = C8 }) {
      return ctor;
    }
  };
  class C8 {
    value() {
      return tag;
    }
  }
  return { read, C: C8 };
});

check('parameter-default', function make9(tag) {
  const read = ({ ctor = C9 } = {}) => ctor;
  class C9 {
    value() {
      return tag;
    }
  }
  return { read, C: C9 };
});

check('assignment-pattern', function make10(tag) {
  const read = () => {
    let ctor;
    const source = [];
    [ctor = C10] = source;
    return ctor;
  };
  class C10 {
    value() {
      return tag;
    }
  }
  return { read, C: C10 };
});
