// Iterator.from (ES2025 §27.1.3.2.1), Iterator.concat and
// Iterator.prototype[Symbol.dispose]: compared byte for byte with Node.

function errName(fn: () => unknown): string {
  try {
    fn();
    return "no error";
  } catch (e: any) {
    return e instanceof TypeError ? "TypeError" : String(e);
  }
}

const I: any = Iterator;

// --- Iterator.from ---------------------------------------------------------
console.log("from.length", I.from.length, "from.name", I.from.name, typeof I.from);
console.log("array", Iterator.from([1, 2, 3]).toArray().join());
console.log("string", Iterator.from("héllo").toArray().join("|"));
console.log("Set", Iterator.from(new Set([4, 5])).map((x: number) => x * 10).toArray().join());

function* gen() {
  yield 1;
  yield 2;
}
const g = gen();
console.log("generator identity", Iterator.from(g) === g);
const arrayIter = [7, 8][Symbol.iterator]();
console.log("array iterator identity", Iterator.from(arrayIter) === arrayIter);
const helper = Iterator.from([1]).map((x: number) => x);
console.log("helper identity", Iterator.from(helper) === helper);

let n = 0;
const plain = {
  next() {
    n++;
    return n <= 3 ? { value: n, done: false } : { value: undefined, done: true };
  },
};
const wrapped = I.from(plain);
const wrapProto = Object.getPrototypeOf(wrapped);
console.log("wrapper proto parent is Iterator.prototype", Object.getPrototypeOf(wrapProto) === Iterator.prototype);
console.log("wrapper proto keys", Object.getOwnPropertyNames(wrapProto).join(), Object.getOwnPropertySymbols(wrapProto).length);
console.log("wrapper instanceof Iterator", wrapped instanceof Iterator);
console.log("wrapper toString", Object.prototype.toString.call(wrapped));
console.log("wrapper next/return", wrapProto.next.name, wrapProto.next.length, wrapProto.return.name, wrapProto.return.length);
console.log("wrapper map", wrapped.map((x: number) => x * 2).toArray().join());

const shared = { value: 42, done: false };
const passthrough = I.from({ next: () => shared });
console.log("next result passthrough", passthrough.next() === shared);

const fromIterable = I.from({
  [Symbol.iterator]() {
    let i = 0;
    return { next: () => (i < 2 ? { value: i++, done: false } : { value: undefined, done: true }) };
  },
});
console.log("@@iterator returning plain next", fromIterable.toArray().join());

const retResult = { value: "r", done: true };
let returnCalls = 0;
const withReturn = I.from({
  next: () => ({ value: 1, done: false }),
  return() {
    returnCalls++;
    return retResult;
  },
});
console.log("wrapper return passthrough", withReturn.return() === retResult, returnCalls);
const noReturn = I.from({ next: () => ({ value: 1, done: false }) }).return();
console.log("wrapper return without underlying", JSON.stringify(noReturn));

console.log("from(5)", errName(() => I.from(5)));
console.log("from(Symbol())", errName(() => I.from(Symbol())));
console.log("from(null)", errName(() => I.from(null)));
console.log("from non-callable @@iterator", errName(() => I.from({ [Symbol.iterator]: 1 })));
console.log("wrapper next bad receiver", errName(() => wrapProto.next.call({})));
console.log("wrapper return bad receiver", errName(() => wrapProto.return.call(helper)));

const f = Iterator.from;
console.log("detached from", f([1, 2]).toArray().join());
console.log("from.call", I.from.call(null, new Set(["a"])).toArray().join());

// --- Iterator.concat -------------------------------------------------------
console.log("concat.length", I.concat.length, "concat.name", I.concat.name, typeof I.concat);
function* three() {
  yield "x";
  yield "y";
}
console.log("concat", Iterator.concat([1, 2], new Set([3]), three(), [[4]]).toArray().map(String).join("|"));
console.log("concat empty", Iterator.concat().toArray().length);
const cat = I.concat([1]);
console.log("concat proto is helper proto", Object.getPrototypeOf(cat) === Object.getPrototypeOf(helper));
const r1 = cat.next();
console.log("concat fresh result", JSON.stringify(r1), JSON.stringify(cat.next()));

let gets = 0;
const counted = {
  get [Symbol.iterator]() {
    gets++;
    return function () {
      return [1][Symbol.iterator]();
    };
  },
};
console.log("concat bad item", errName(() => I.concat(counted, 5, counted)), gets);
console.log("concat no @@iterator", errName(() => I.concat({})));
console.log("concat string primitive", errName(() => I.concat("ab")));

let closed = 0;
const closable = {
  [Symbol.iterator]() {
    return {
      next: () => ({ value: "c", done: false }),
      return() {
        closed++;
        return {};
      },
    };
  },
};
const catClose = I.concat([0], closable, [9]);
console.log("before start return", JSON.stringify(I.concat(closable).return()), closed);
console.log(catClose.next().value, catClose.next().value, closed);
console.log("return closes active", JSON.stringify(catClose.return()), closed);
console.log("after return", JSON.stringify(catClose.next()), closed);

// --- Iterator.prototype[Symbol.dispose] -----------------------------------
const dispose = (Iterator.prototype as any)[Symbol.dispose];
console.log("dispose", typeof dispose, dispose.name, dispose.length);
let disposed = 0;
const disposable = I.from({
  next: () => ({ value: 1, done: false }),
  return() {
    disposed++;
    return {};
  },
});
console.log("dispose result", disposable[Symbol.dispose](), disposed);
console.log("dispose without return", dispose.call({}));
const genToDispose = gen();
genToDispose.next();
genToDispose[Symbol.dispose]();
console.log("generator after dispose", JSON.stringify(genToDispose.next()));

// --- %IteratorHelperPrototype% surface ------------------------------------
const helperProto = Object.getPrototypeOf(helper);
console.log("helper proto names", Object.getOwnPropertyNames(helperProto).join());
console.log("typeof helper.throw", typeof (helper as any).throw);
console.log("helper.throw()", errName(() => (helper as any).throw()));
