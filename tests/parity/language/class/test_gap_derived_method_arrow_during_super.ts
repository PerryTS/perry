// A base constructor that calls an overridden method runs the derived
// class's method while the derived constructor is still inside `super()`.
// An arrow in that METHOD captures the method's receiver, which is
// initialized: it must not be checked against the derived constructor's
// pending `this` binding. (Next.js: Server's constructor calls
// NextNodeServer#getinterceptionRoutePatterns -> getRoutesManifest ->
// tracer.trace(..., () => this...), which threw "Must call super
// constructor".) Arrows written in the derived constructor itself keep the
// TDZ: reading `this` before `super()` still throws.
// An opaque tracer (like Next's NextTracerImpl) so the arrows stay real
// closures invoked through a call.
const tracer: { trace<T>(fn: () => T): T } = JSON.parse("{}");
tracer.trace = function (fn) {
  return fn();
};
function trace<T>(fn: () => T): T {
  return tracer.trace(fn);
}

class Base {
  patterns: string[];
  opts: { dir: string };
  constructor(opts: { dir: string }) {
    this.opts = opts;
    this.patterns = this.getPatterns();
  }
  getPatterns(): string[] {
    return [];
  }
}

class Derived extends Base {
  distDir = ".next";
  onReady: () => string;
  constructor(opts: { dir: string }) {
    // An arrow in the constructor that reads `this` makes the constructor
    // publish its pending binding for arrows to check.
    const late = () => this.distDir;
    super(opts);
    this.onReady = late;
    console.log("ctor arrow after super", late());
  }
  getManifest(): string {
    return trace(() => (this.opts.dir + "/" + String(this.distDir)));
  }
  getPatterns(): string[] {
    const m = trace(() => this.getManifest());
    return [m, trace(() => typeof this)];
  }
}

const d = new Derived({ dir: "/app" });
console.log(JSON.stringify(d.patterns), d.getManifest());

class EarlyArrow extends Base {
  constructor() {
    const peek = () => this;
    try {
      peek();
      console.log("early arrow: no throw");
    } catch (e: any) {
      console.log("early arrow:", e.constructor.name);
    }
    super({ dir: "/x" });
    console.log("after super:", peek() === this);
  }
}
new EarlyArrow();

class Twice extends Base {
  constructor() {
    super({ dir: "/y" });
    const again = () => super({ dir: "/z" } as any);
    try {
      again();
      console.log("second super: no throw");
    } catch (e: any) {
      console.log("second super:", e.constructor.name);
    }
  }
}
new Twice();
