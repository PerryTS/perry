// Object.assign uses Set(target, key, value, true), including inherited descriptors.
const log: string[] = [];
const proto = { set k(v: any) { log.push("setter " + v); } };
const target = Object.create(proto);
Object.assign(target, { k: 1 });
console.log("inherited setter:", log.join(","), Object.prototype.hasOwnProperty.call(target, "k"));

function attempt(label: string, target: any, source: any) {
  try {
    Object.assign(target, source);
    console.log(label, "no throw");
  } catch (e: any) {
    console.log(label, e.constructor.name);
  }
}
attempt("read-only:", Object.create(Object.defineProperty({}, "k", { value: 0, writable: false })), { k: 2 });
attempt("getter-only:", Object.create({ get k() { return 0; } }), { k: 3 });
attempt("own read-only:", Object.defineProperty({}, "k", { value: 0 }), { k: 3 });
attempt("own getter-only:", { get k() { return 0; } }, { k: 3 });
attempt("boxed string index:", "abc", { 0: "x" });
attempt("non-extensible data:", Object.preventExtensions({}), { k: 3 });

// A setter may run on a non-extensible receiver without defining an own key.
const sealed = Object.preventExtensions(Object.create(Object.create(proto)));
attempt("non-extensible setter:", sealed, { k: 4 });
console.log("deep setter:", log.join(","), Object.prototype.hasOwnProperty.call(sealed, "k"));

const writableProto = { k: 0 };
const writable = Object.create(writableProto);
Object.assign(writable, { k: 5 });
console.log("writable:", writable.k, writableProto.k, Object.prototype.hasOwnProperty.call(writable, "k"));

const receiverProto = { set k(v: any) { this.saved = v; } };
const receiver = Object.create(receiverProto);
Object.assign(receiver, { k: 6 });
console.log("setter receiver:", receiver.saved, Object.prototype.hasOwnProperty.call(receiverProto, "saved"));

// Own writable data shadows an inherited accessor or read-only property.
const shadow = Object.create(proto);
Object.defineProperty(shadow, "k", { value: 6, writable: true, enumerable: true });
Object.assign(shadow, { k: 7 });
console.log("shadow:", shadow.k, log.join(","));

const symbol = Symbol("key");
const symbolProto: any = {};
Object.defineProperty(symbolProto, symbol, { set(v: any) { log.push("symbol " + v); } });
const symbolTarget = Object.create(symbolProto);
const symbolSource: any = {};
symbolSource[symbol] = 8;
Object.assign(symbolTarget, symbolSource);
console.log("symbol setter:", log.join(","), Object.prototype.hasOwnProperty.call(symbolTarget, symbol));
const symbolReadonly: any = {};
Object.defineProperty(symbolReadonly, symbol, { value: 0, writable: false });
attempt("symbol read-only:", Object.create(symbolReadonly), symbolSource);
const symbolGetter: any = {};
Object.defineProperty(symbolGetter, symbol, { get() { return 0; } });
attempt("symbol getter-only:", Object.create(symbolGetter), symbolSource);

// Recheck each snapshotted source key after earlier target setters have run.
const source: any = { a: 1, b: 2, c: 3 };
const deleting: any = { set a(v: any) { delete source.b; } };
Object.assign(deleting, source);
console.log("deleted source key:", Object.keys(deleting).join(","), deleting.c);

const changing: any = { a: 1, b: 2, c: 3 };
const changingTarget: any = { set a(v: any) {
  Object.defineProperty(changing, "b", { enumerable: false });
  Object.defineProperty(changing, "c", { get() { return 9; }, enumerable: true });
} };
Object.assign(changingTarget, changing);
console.log("changed source descriptor:", Object.keys(changingTarget).join(","), changingTarget.c);

// Keys are snapshotted once, including symbols, before any getters/setters run.
const added = Symbol("added");
const addingSource: any = { a: 1 };
const addingTarget: any = { set a(v: any) { addingSource.extra = 2; addingSource[added] = 3; } };
Object.assign(addingTarget, addingSource);
console.log("added source keys:", Object.keys(addingTarget).join(","), Object.getOwnPropertySymbols(addingTarget).length);
