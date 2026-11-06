// Symbol-keyed instance methods of a class declared inside a function whose
// class evaluation is per call (computed fields, private elements, dynamic
// heritage) live on that evaluation's prototype — undici's PoolBase pattern:
// `var PoolBase = class extends DispatcherBase { [kQueue] = []; [kOnDrain](...) {} }`.
function withComputedField() {
  const kMethod = Symbol("method");
  const kField = Symbol("field");
  class A {
    [kField] = 1;
    [kMethod](x: number) {
      return x + (this as any)[kField];
    }
  }
  const a: any = new A();
  return [typeof a[kMethod], a[kMethod](1), typeof (A.prototype as any)[kMethod], a[kMethod].bind(a, 2)()];
}

function withPrivateField() {
  const kMethod = Symbol("method");
  class A {
    #p = 3;
    [kMethod]() {
      return this.#p;
    }
  }
  const a: any = new A();
  return [typeof a[kMethod], a[kMethod]()];
}

function withDynamicParent(Base: any) {
  const kMethod = Symbol("method");
  class A extends Base {
    [kMethod]() {
      return "child";
    }
  }
  const a: any = new A();
  return [typeof a[kMethod], a[kMethod](), a.base()];
}

function twoEvaluations() {
  const kMethod = Symbol("method");
  const kField = Symbol("field");
  const make = (n: number) =>
    class {
      [kField] = n;
      [kMethod]() {
        return (this as any)[kField];
      }
    };
  const A = make(4);
  const B = make(5);
  return [(new A() as any)[kMethod](), (new B() as any)[kMethod]()];
}

console.log(JSON.stringify(withComputedField()));
console.log(JSON.stringify(withPrivateField()));
console.log(JSON.stringify(withDynamicParent(class { base() { return "base"; } })));
console.log(JSON.stringify(twoEvaluations()));
