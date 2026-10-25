// A closure that reads a class declared after it must read that binding, not
// the class's shared template, wherever in the closure the read sits. This
// probe puts the read in the positions that only run something: the computed
// name of an object or class member (method, accessor, field, static
// member), a class field initializer, a static block, the arguments of a
// `super(...)` call, a `super[...]` access, a template tag, an optional call
// and a constructor parameter default. Each case builds two scopes and checks
// that each closure read its own scope's class. Every line is compared with
// node. Each case runs on its own so that one failure does not hide the
// others (a case that throws prints `threw`).

function outcome(label, value) {
  console.log(label, typeof value === 'string' ? value : JSON.stringify(value));
}

function check(label, make) {
  try {
    const a = make('a');
    const b = make('b');
    outcome(label, [a.read() === a.C, b.read() === b.C]);
  } catch (e) {
    outcome(label, 'threw ' + (e && e.name));
  }
}

check('object-method-key', function make1(tag) {
  const read = () => {
    const seen = [];
    const obj = { [(seen.push(C1), 'm')]() {} };
    return seen[0];
  };
  class C1 extends Error {
    tag() {
      return tag;
    }
  }
  return { read, C: C1 };
});

check('object-getter-key', function make2(tag) {
  const read = () => {
    const seen = [];
    const obj = { get [(seen.push(C2), 'g')]() { return 1; } };
    return seen[0];
  };
  class C2 extends Error {
    tag() {
      return tag;
    }
  }
  return { read, C: C2 };
});

check('object-setter-key', function make3(tag) {
  const read = () => {
    const seen = [];
    const obj = { set [(seen.push(C3), 's')](v) {} };
    return seen[0];
  };
  class C3 extends Error {
    tag() {
      return tag;
    }
  }
  return { read, C: C3 };
});

check('object-value-key', function make4(tag) {
  const read = () => {
    const seen = [];
    const obj = { [(seen.push(C4), 'k')]: 1 };
    return seen[0];
  };
  class C4 extends Error {
    tag() {
      return tag;
    }
  }
  return { read, C: C4 };
});

check('class-method-key', function make5(tag) {
  const read = () => {
    const seen = [];
    class K { [(seen.push(C5), 'm')]() {} }
    return seen[0];
  };
  class C5 extends Error {
    tag() {
      return tag;
    }
  }
  return { read, C: C5 };
});

check('class-static-method-key', function make6(tag) {
  const read = () => {
    const seen = [];
    class K { static [(seen.push(C6), 'm')]() {} }
    return seen[0];
  };
  class C6 extends Error {
    tag() {
      return tag;
    }
  }
  return { read, C: C6 };
});

check('class-getter-key', function make7(tag) {
  const read = () => {
    const seen = [];
    class K { get [(seen.push(C7), 'g')]() { return 1; } }
    return seen[0];
  };
  class C7 extends Error {
    tag() {
      return tag;
    }
  }
  return { read, C: C7 };
});

check('class-setter-key', function make8(tag) {
  const read = () => {
    const seen = [];
    class K { set [(seen.push(C8), 's')](v) {} }
    return seen[0];
  };
  class C8 extends Error {
    tag() {
      return tag;
    }
  }
  return { read, C: C8 };
});

check('class-field-key', function make9(tag) {
  const read = () => {
    const seen = [];
    class K { [(seen.push(C9), 'p')] = 1; }
    return seen[0];
  };
  class C9 extends Error {
    tag() {
      return tag;
    }
  }
  return { read, C: C9 };
});

check('class-static-field-key', function make10(tag) {
  const read = () => {
    const seen = [];
    class K { static [(seen.push(C10), 'p')] = 1; }
    return seen[0];
  };
  class C10 extends Error {
    tag() {
      return tag;
    }
  }
  return { read, C: C10 };
});

check('class-field-initializer', function make11(tag) {
  const read = () => {
    const seen = [];
    class K { p = seen.push(C11); } new K();
    return seen[0];
  };
  class C11 extends Error {
    tag() {
      return tag;
    }
  }
  return { read, C: C11 };
});

check('class-static-block', function make12(tag) {
  const read = () => {
    const seen = [];
    class K { static { seen.push(C12); } }
    return seen[0];
  };
  class C12 extends Error {
    tag() {
      return tag;
    }
  }
  return { read, C: C12 };
});

check('class-super-arguments', function make13(tag) {
  const read = () => {
    const seen = [];
    class B { constructor(x) {} } class K extends B { constructor() { super(seen.push(C13)); } } new K();
    return seen[0];
  };
  class C13 extends Error {
    tag() {
      return tag;
    }
  }
  return { read, C: C13 };
});

check('object-super-computed', function make14(tag) {
  const read = () => {
    const seen = [];
    const obj = { m() { return super[(seen.push(C14), 'toString')]; } }; obj.m();
    return seen[0];
  };
  class C14 extends Error {
    tag() {
      return tag;
    }
  }
  return { read, C: C14 };
});

check('tagged-template-tag', function make15(tag) {
  const read = () => {
    const seen = [];
    const t = (seen.push(C15), String.raw)`x`;
    return seen[0];
  };
  class C15 extends Error {
    tag() {
      return tag;
    }
  }
  return { read, C: C15 };
});

check('optional-call-arguments', function make16(tag) {
  const read = () => {
    const seen = [];
    seen?.push(C16);
    return seen[0];
  };
  class C16 extends Error {
    tag() {
      return tag;
    }
  }
  return { read, C: C16 };
});

check('optional-computed-member', function make17(tag) {
  const read = () => {
    const seen = [];
    seen?.[(seen.push(C17), 'length')];
    return seen[0];
  };
  class C17 extends Error {
    tag() {
      return tag;
    }
  }
  return { read, C: C17 };
});

check('constructor-parameter-default', function make18(tag) {
  const read = () => {
    const seen = [];
    class K { constructor(p = seen.push(C18)) {} } new K();
    return seen[0];
  };
  class C18 extends Error {
    tag() {
      return tag;
    }
  }
  return { read, C: C18 };
});
