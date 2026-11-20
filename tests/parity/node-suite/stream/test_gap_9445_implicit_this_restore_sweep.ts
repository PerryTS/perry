// #9445: every runtime site that binds `this` for a callback must not corrupt
// the CALLER's `this` across an evacuating young-gen minor.
//
// The shape (#9417 / PR #9444 fixed it for the accessor sites; this is the
// sweep of the ~120 others):
//
//     let prev = js_implicit_this_set(receiver);
//     … user code, which allocates …
//     js_implicit_this_set(prev);          // pre-collection address
//
// `prev` is the caller's receiver in a bare Rust local. A copying minor inside
// the window relocates that object and rewrites every slot the collector can
// see — a Rust local is not one — so the restore reinstalls a retired
// from-space address as the caller's `this`. Nothing faults: the caller's next
// `this.<field>` reads `undefined` off the recycled cell and the member access
// after it throws `Cannot read properties of undefined (reading 'def')`.
//
// Each case below is a factory returning a fresh (young, escaping) object whose
// `run` is a `function`-expression method — one that reads `this` dynamically
// off the implicit-`this` cell, not from a captured slot. `run` drives ONE
// runtime site with a callback that allocates past the nursery, then reads
// `this.inner.def`. The callback is reached through a syntax or a typed
// builtin that lowers to a direct runtime call, so no compiled-code
// save/restore sits between the runtime site and `run`'s next `this` read.
//
// Pre-fix each case prints a non-zero `bad=` count, deterministically and with
// no GC env knobs. Node prints 0 for every line.

import { Writable, Readable, Transform } from "node:stream";

const N = 1500;

function churn(): number {
  const tmp: any[] = [];
  for (let k = 0; k < 480; k++) tmp.push({ k: k, s: "t" + k, pad: [k, k + 1] });
  return tmp.length;
}

function check(name: string, factory: (i: number) => any): void {
  let bad = 0;
  const notes: string[] = [];
  for (let i = 0; i < N; i++) {
    const c: any = factory(i);
    const want = "c" + i + ":480";
    let got: any;
    try {
      got = c.run();
    } catch (e: any) {
      got = "THREW:" + (e && e.message);
    }
    if (got !== want) {
      bad++;
      if (bad <= 2) notes.push("[" + i + " got=" + String(got) + "]");
    }
  }
  console.log(name + " bad=" + bad + notes.join(""));
}

function host(i: number, run: (this: any) => string): any {
  return { id: i, inner: { def: "c" + i }, run: run };
}

function userIterable(onNext: () => void): any {
  return {
    [Symbol.iterator]: function () {
      let done = false;
      return {
        next: function () {
          if (done) return { done: true, value: undefined };
          done = true;
          onNext();
          return { done: false, value: 1 };
        },
      };
    },
  };
}

// --- collections ---------------------------------------------------------

check("map_forEach", function (i) {
  const m = new Map<number, number>([[1, 1]]);
  return host(i, function (this: any) {
    let n = 0;
    m.forEach(function () {
      n = churn();
    });
    return this.inner.def + ":" + n;
  });
});

check("set_forEach", function (i) {
  const s = new Set<number>([1]);
  return host(i, function (this: any) {
    let n = 0;
    s.forEach(function () {
      n = churn();
    });
    return this.inner.def + ":" + n;
  });
});

check("urlsearchparams_forEach", function (i) {
  const p = new URLSearchParams("a=1");
  return host(i, function (this: any) {
    let n = 0;
    p.forEach(function () {
      n = churn();
    });
    return this.inner.def + ":" + n;
  });
});

// --- events --------------------------------------------------------------

check("event_target_dispatch", function (i) {
  const et = new EventTarget();
  let n = 0;
  et.addEventListener("x", function () {
    n = churn();
  });
  return host(i, function (this: any) {
    et.dispatchEvent(new Event("x"));
    return this.inner.def + ":" + n;
  });
});

// --- property keys, accessors, ToPrimitive --------------------------------

check("to_property_key_toString", function (i) {
  const target: any = { k: 1 };
  return host(i, function (this: any) {
    let n = 0;
    const key: any = {
      toString: function () {
        n = churn();
        return "k";
      },
    };
    const v = target[key];
    return this.inner.def + ":" + (v === 1 ? n : -1);
  });
});

check("defineProperty_setter", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    const o: any = {};
    Object.defineProperty(o, "s", {
      set: function (_v: any) {
        n = churn();
      },
      configurable: true,
    });
    o.s = 1;
    return this.inner.def + ":" + n;
  });
});

check("function_object_getter", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    const f: any = function () {};
    Object.defineProperty(f, "g", {
      get: function () {
        n = churn();
        return 1;
      },
      configurable: true,
    });
    const v = f.g;
    return this.inner.def + ":" + (v === 1 ? n : -1);
  });
});

check("valueOf_to_primitive", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    const o: any = {
      valueOf: function () {
        n = churn();
        return 1;
      },
    };
    const v = o + 1;
    return this.inner.def + ":" + (v === 2 ? n : -1);
  });
});

check("toString_template", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    const o: any = {
      toString: function () {
        n = churn();
        return "s";
      },
    };
    const v = `${o}`;
    return this.inner.def + ":" + (v === "s" ? n : -1);
  });
});

check("symbol_toPrimitive", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    const o: any = {
      [Symbol.toPrimitive]: function (_hint: string) {
        n = churn();
        return 1;
      },
    };
    const v = +o;
    return this.inner.def + ":" + (v === 1 ? n : -1);
  });
});

// --- JSON ----------------------------------------------------------------

check("json_stringify_getter", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    const o: any = {};
    Object.defineProperty(o, "g", {
      get: function () {
        n = churn();
        return 1;
      },
      enumerable: true,
      configurable: true,
    });
    const s = JSON.stringify(o);
    return this.inner.def + ":" + (s === '{"g":1}' ? n : -1);
  });
});

check("json_stringify_toJSON", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    const o: any = {
      toJSON: function () {
        n = churn();
        return 1;
      },
    };
    const s = JSON.stringify({ o: o });
    return this.inner.def + ":" + (s === '{"o":1}' ? n : -1);
  });
});

check("json_stringify_replacer", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    const s = JSON.stringify({ a: 1 }, function (_k: string, v: any) {
      n = churn();
      return v;
    });
    return this.inner.def + ":" + (s === '{"a":1}' ? n : -1);
  });
});

check("json_parse_reviver", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    const o = JSON.parse('{"a":1}', function (_k: string, v: any) {
      n = churn();
      return v;
    });
    return this.inner.def + ":" + (o.a === 1 ? n : -1);
  });
});

// --- iteration protocol --------------------------------------------------

check("for_of_user_iterator", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    let sum = 0;
    for (const v of userIterable(function () {
      n = churn();
    })) {
      sum += v;
    }
    return this.inner.def + ":" + (sum === 1 ? n : -1);
  });
});

check("spread_user_iterator", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    const arr = [
      ...userIterable(function () {
        n = churn();
      }),
    ];
    return this.inner.def + ":" + (arr.length === 1 ? n : -1);
  });
});

check("destructure_user_iterator", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    const [first] = userIterable(function () {
      n = churn();
    });
    return this.inner.def + ":" + (first === 1 ? n : -1);
  });
});

check("array_from_user_iterator", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    const arr = Array.from(
      userIterable(function () {
        n = churn();
      }),
    );
    return this.inner.def + ":" + (arr.length === 1 ? n : -1);
  });
});

// --- Proxy / Reflect -----------------------------------------------------

check("proxy_get_trap", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    const p: any = new Proxy(
      {},
      {
        get: function (_t: any, _k: any) {
          n = churn();
          return 1;
        },
      },
    );
    const v = p.x;
    return this.inner.def + ":" + (v === 1 ? n : -1);
  });
});

check("proxy_set_trap", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    const p: any = new Proxy(
      {},
      {
        set: function (_t: any, _k: any, _v: any) {
          n = churn();
          return true;
        },
      },
    );
    p.x = 1;
    return this.inner.def + ":" + n;
  });
});

check("proxy_apply_trap", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    const p: any = new Proxy(function () {}, {
      apply: function () {
        n = churn();
        return 1;
      },
    });
    const v = p();
    return this.inner.def + ":" + (v === 1 ? n : -1);
  });
});

check("proxy_construct_trap", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    const p: any = new Proxy(function () {}, {
      construct: function () {
        n = churn();
        return { ok: 1 };
      },
    });
    const v = new p();
    return this.inner.def + ":" + (v.ok === 1 ? n : -1);
  });
});

check("reflect_apply", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    const v = Reflect.apply(
      function () {
        n = churn();
        return 1;
      },
      null,
      [],
    );
    return this.inner.def + ":" + (v === 1 ? n : -1);
  });
});

check("reflect_get_receiver_getter", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    const o: any = {
      get g() {
        n = churn();
        return 1;
      },
    };
    const v = Reflect.get(o, "g", o);
    return this.inner.def + ":" + (v === 1 ? n : -1);
  });
});

// --- functions -----------------------------------------------------------

check("bound_function_call", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    const b = function (this: any) {
      n = churn();
      return 1;
    }.bind({});
    const v = b();
    return this.inner.def + ":" + (v === 1 ? n : -1);
  });
});

check("string_replace_callback", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    const s = "abc".replace(/b/, function () {
      n = churn();
      return "x";
    });
    return this.inner.def + ":" + (s === "axc" ? n : -1);
  });
});

check("using_dispose", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    {
      using _r = {
        [Symbol.dispose]: function () {
          n = churn();
        },
      };
    }
    return this.inner.def + ":" + n;
  });
});

// --- node:stream user hooks ----------------------------------------------

check("writable_write", function (i) {
  let n = 0;
  const w = new Writable({
    write(_chunk, _enc, cb) {
      n = churn();
      cb();
    },
  });
  return host(i, function (this: any) {
    w.write("x");
    return this.inner.def + ":" + n;
  });
});

check("writable_writev", function (i) {
  let n = 0;
  const w = new Writable({
    write(_chunk, _enc, cb) {
      cb();
    },
    writev(_chunks, cb) {
      n = churn();
      cb();
    },
  });
  return host(i, function (this: any) {
    w.cork();
    w.write("a");
    w.write("b");
    w.uncork();
    return this.inner.def + ":" + n;
  });
});

check("transform_transform", function (i) {
  let n = 0;
  const t = new Transform({
    transform(chunk, _enc, cb) {
      n = churn();
      cb(null, chunk);
    },
  });
  t.on("data", function () {});
  return host(i, function (this: any) {
    t.write("x");
    return this.inner.def + ":" + n;
  });
});

check("writable_construct", function (i) {
  return host(i, function (this: any) {
    let n = 0;
    new Writable({
      construct(cb) {
        n = churn();
        cb();
      },
      write(_chunk, _enc, cb) {
        cb();
      },
    });
    // node defers `_construct` to nextTick; only the caller's `this` matters.
    return this.inner.def + ":" + (n === 0 || n === 480 ? 480 : -1);
  });
});

check("readable_read", function (i) {
  let n = 0;
  const r = new Readable({
    read() {
      n = churn();
      this.push(null);
    },
  });
  return host(i, function (this: any) {
    r.read();
    return this.inner.def + ":" + n;
  });
});

check("writable_final", function (i) {
  let n = 0;
  const w = new Writable({
    write(_chunk, _enc, cb) {
      cb();
    },
    final(cb) {
      n = churn();
      cb();
    },
  });
  return host(i, function (this: any) {
    w.end();
    return this.inner.def + ":" + n;
  });
});

check("transform_flush", function (i) {
  let n = 0;
  const t = new Transform({
    transform(chunk, _enc, cb) {
      cb(null, chunk);
    },
    flush(cb) {
      n = churn();
      cb();
    },
  });
  t.on("data", function () {});
  return host(i, function (this: any) {
    t.end();
    return this.inner.def + ":" + n;
  });
});
