// #11692: a `#private` method called through a subclass whose parent is a
// runtime value (`class extends Base {}` built by a helper) must keep
// resolving after any prototype mutation elsewhere in the program. #11667
// derived "this declared prototype member was deleted" from "its string key is
// not on the prototype object once the prototype guards are invalidated", and a
// private method is never a string key there, so the first unrelated
// `Object.setPrototypeOf` made every such call throw `#<...#m> is not a
// function` (node-redis: `this.#validateOptions` in `new RedisClient`).

class Client {
  opts: any;
  static #made = 0;
  constructor(o: any) {
    this.#validate(o);
    this.opts = this.#init(o);
    Client.#made = Client.#count() + 1;
  }
  #validate(o: any) {
    if (o && o.bad) throw new TypeError("bad options");
  }
  #init(o: any) {
    return { ...o, ok: true };
  }
  static #count() {
    return Client.#made;
  }
  static made() {
    return Client.#count();
  }
}

function attach(Base: any) {
  return class extends Base {};
}

const Sub = attach(Client);
console.log("before:", new Sub({ a: 1 }).opts.ok, Client.made());

// An unrelated prototype mutation invalidates every prototype fast guard.
const unrelated: any = {};
Object.setPrototypeOf(unrelated, { x: 1 });

console.log("after setPrototypeOf:", new Sub({ a: 2 }).opts.ok, Client.made());
console.log("direct:", new Client({}).opts.ok, Client.made());
try {
  new Sub({ bad: true });
} catch (e: any) {
  console.log("validate still runs:", e instanceof TypeError, e.message);
}

// Deleting an ordinary method is still observed through the same path.
class WithMethod {
  #secret() {
    return "s";
  }
  m() {
    return "m";
  }
  reveal() {
    return this.#secret();
  }
}
const Sub2 = attach(WithMethod);
const w: any = new Sub2();
console.log("method:", w.m(), w.reveal());
delete (WithMethod.prototype as any).m;
console.log("deleted method:", typeof w.m, "private still:", w.reveal());
