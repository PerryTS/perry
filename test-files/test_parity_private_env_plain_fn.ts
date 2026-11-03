// A plain function created in a class body keeps that class evaluation's
// private environment: the private name resolves against the evaluation that
// created the function, whichever evaluation is running when it is called.
function make(tag: string) {
  return class {
    #s = tag;
    static read: (o: any) => string;
    static {
      this.read = function (o: any) {
        return o.#s;
      };
    }
    static create() {
      return new this();
    }
  };
}
const A = make("a");
const B = make("b");
const a = A.create();
const b = B.create();
console.log(A.read(a), B.read(b));
for (const [cls, obj] of [[A, b], [B, a]] as const) {
  try {
    console.log(cls.read(obj));
  } catch (e) {
    console.log(e instanceof TypeError ? "TypeError" : String(e));
  }
}
const C = make("c");
const c = C.create();
console.log(A.read.call(undefined, a), C.read(c), A.read(a));
