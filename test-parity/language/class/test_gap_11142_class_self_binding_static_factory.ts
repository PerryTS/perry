// #11142: a per-evaluation class's own name inside its body is that evaluation.

function attachConfig(opts: { BaseClass: any; tag: string }): any {
  return class extends opts.BaseClass {
    configTag() {
      return opts.tag;
    }
  };
}

function body(tag: string) {
  class RC {
    #s = tag;
    static #made = 0;
    static me = RC;
    static make() {
      RC.#made++;
      const C = attachConfig({ BaseClass: RC, tag: "static" });
      return new C();
    }
    static makeFromThis() {
      const C = attachConfig({ BaseClass: this, tag: "this" });
      return new C();
    }
    static makeInArrow() {
      const build = () => new (attachConfig({ BaseClass: RC, tag: "arrow" }))();
      return build();
    }
    static create() {
      return new RC();
    }
    static viaStaticCall() {
      return RC.make();
    }
    static self() {
      return RC;
    }
    static made() {
      return RC.#made;
    }
    static has(o: any) {
      return #s in o;
    }
    selfFromInstance() {
      return RC;
    }
    isRC(o: any) {
      return o instanceof RC;
    }
    read() {
      return this.#s;
    }
  }
  return RC;
}

const A: any = body("a");
const B: any = body("b");

console.log("self", A.self() === A, A.me === A, new A().selfFromInstance() === A, A.self() === B);
console.log("self-b", B.self() === B, B.me === B, B.create() instanceof B, B.create() instanceof A);
for (const [label, v] of [
  ["static", A.make()],
  ["this", A.makeFromThis()],
  ["arrow", A.makeInArrow()],
  ["create", A.create()],
  ["nested", A.viaStaticCall()],
] as [string, any][]) {
  console.log(
    label,
    v.read(),
    A.has(v),
    v instanceof A,
    new A().isRC(v),
    typeof v.configTag === "function" ? v.configTag() : "-",
  );
}
console.log("made", A.made(), B.made());
// Brands and heritage stay per evaluation.
const fromA = A.make();
console.log("cross", B.has(fromA), fromA instanceof B, new B().isRC(fromA));
try {
  B.prototype.read.call(fromA);
  console.log("cross read: no throw");
} catch (e) {
  console.log("cross read:", (e as Error).constructor.name);
}

// Dynamic heritage without private elements: the class is per-evaluation too.
function mixin(Base: any) {
  class M extends Base {
    static make() {
      return new (attachConfig({ BaseClass: M, tag: "mixin" }))();
    }
    static self() {
      return M;
    }
  }
  return M;
}
class Root {}
const M1: any = mixin(Root);
const M2: any = mixin(Root);
const m = M1.make();
console.log("mixin", M1.self() === M1, m instanceof M1, m instanceof M2, m instanceof Root, m.configTag());

// @redis/client 6.1.0 shape: `RedisClient.create` builds its subclass in the
// class's own static `factory`.
class Emitter {
  on() {
    return this;
  }
}
function redisModule(EventEmitter: any) {
  class RedisClient extends EventEmitter {
    #options: any;
    static factory(config: any) {
      return attachConfig({ BaseClass: RedisClient, tag: config.name });
    }
    static create(options: any) {
      return new (RedisClient.factory(options))(options);
    }
    constructor(options: any) {
      super();
      this.#options = options;
    }
    get options() {
      return this.#options;
    }
    static isClient(o: any) {
      return #options in o;
    }
  }
  return RedisClient;
}
const RedisClient: any = redisModule(Emitter);
const client = RedisClient.create({ name: "redis" });
console.log(
  "redis",
  client instanceof RedisClient,
  RedisClient.isClient(client),
  client.options.name,
  client.configTag(),
  client instanceof Emitter,
);

// The self-binding also holds this evaluation inside async and generator
// bodies (the owner local is boxed by the async/generator transform), and in a
// loop body that evaluates the declaration once per iteration. (A module-level
// loop's self-binding is one module global, so after the loop an earlier
// iteration's static methods see the last evaluation. Not asserted here.)
async function loadAsync(tag: string) {
  await null;
  class AR {
    #s = tag;
    static self() {
      return AR;
    }
    static make() {
      return new (attachConfig({ BaseClass: AR, tag: "async" }))();
    }
  }
  return AR;
}
function* genClasses(tag: string) {
  class GR {
    #s = tag;
    static self() {
      return GR;
    }
  }
  yield GR;
}
const seen: any[] = [];
for (let i = 0; i < 2; i++) {
  class LR {
    #i = i;
    static self() {
      return LR;
    }
  }
  seen.push(LR);
  console.log("loop", i, LR.self() === LR, new LR() instanceof LR);
}
console.log("loop distinct", seen[0] !== seen[1]);
const [GR]: any[] = [...genClasses("g")];
console.log("generator", GR.self() === GR);
loadAsync("a").then((AR: any) => {
  const made = AR.make();
  console.log("async", AR.self() === AR, made instanceof AR, made.configTag());
});
