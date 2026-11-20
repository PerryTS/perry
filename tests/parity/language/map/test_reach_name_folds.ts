// A fold chosen by the receiver's NAME is also an ordinary method call.
// `wr`, `reg` below are genuine WeakRef / FinalizationRegistry bindings at
// module level, so HIR lowers every `wr.deref()`, `reg.register(...)` and
// `reg.unregister(...)` in this module to the intrinsic node, whatever a
// binding of that name holds at the site. The runtime helper tests the
// receiver's kind and hands any other receiver to ordinary dispatch, so the
// receiver's own method runs. The whole-program solve modelled the intrinsic
// only: it called these method bodies inactive while they ran (check mode
// `PERRY_VERIFY_FACTS=1`: "inactive body: body entered"; with
// `PERRY_FACTS_DCE=1` the removed body trapped).
const target = { tag: "T" };
const wr = new WeakRef(target);
const reg = new FinalizationRegistry<number>(() => {});
console.log("genuine:", (wr.deref() as any).tag);
reg.register(target, 1, target);
console.log("genuine-unregister:", reg.unregister(target));

class Cache {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
  deref(): number {
    return this.v * 10;
  }
  register(a: object, b: number): string {
    return "user-register:" + b + ":" + this.v;
  }
  unregister(a: object): string {
    return "user-unregister:" + this.v;
  }
}

function derefLiteral(): unknown {
  const wr = { deref: () => 40 };
  return wr.deref();
}
function derefClass(): unknown {
  const wr = new Cache(4);
  return wr.deref();
}
function derefArrayProp(): unknown {
  const wr: any = [1, 2, 3];
  wr.deref = () => 41;
  return wr.deref();
}
function derefParam(wr: { deref: () => number }): unknown {
  return wr.deref();
}
function derefThis(): unknown {
  const wr = {
    n: 7,
    deref() {
      return this.n + 1;
    },
  };
  return wr.deref();
}
// The method's result is the expression's value: a function it returns runs.
function derefReturnsFunction(): unknown {
  const wr = { deref: () => () => "inner" };
  return (wr.deref() as any)();
}
console.log("deref-literal:", derefLiteral());
console.log("deref-class:", derefClass());
console.log("deref-array-prop:", derefArrayProp());
console.log("deref-param:", derefParam({ deref: () => 42 }));
console.log("deref-this:", derefThis());
console.log("deref-returns-function:", derefReturnsFunction());

function registerLiteral(): unknown {
  const reg = { register: (a: object, b: number, c?: object) => "lit-register:" + b + ":" + (c === undefined) };
  return reg.register(target, 3);
}
function registerClass(): unknown {
  const reg = new Cache(1);
  return reg.register(target, 5);
}
function unregisterLiteral(): unknown {
  const reg = { unregister: (a: object) => "lit-unregister:" + (a === target) };
  return reg.unregister(target);
}
function unregisterClass(): unknown {
  const reg = new Cache(2);
  return reg.unregister(target);
}
function registerParam(reg: { register: (a: object, b: number) => string }): unknown {
  return reg.register(target, 9);
}
console.log("register-literal:", registerLiteral());
console.log("register-class:", registerClass());
console.log("unregister-literal:", unregisterLiteral());
console.log("unregister-class:", unregisterClass());
console.log("register-param:", registerParam({ register: (a, b) => "param-register:" + b }));

// Controls: the genuine receivers still answer after the foreign ones ran,
// and a receiver without the method throws as in Node.
console.log("genuine-again:", (wr.deref() as any).tag, wr.deref() === target);
function derefMissing(): string {
  const wr: any = { other: 1 };
  try {
    wr.deref();
    return "no throw";
  } catch (e) {
    return (e as Error).name;
  }
}
console.log("deref-missing:", derefMissing());
