// A registry key may spell anything: `Symbol.for(key)` is always a registered
// symbol, never a well-known one.

function keyFor(key: string): string {
  return String(Symbol.keyFor(Symbol.for(key)));
}

// --- literal, constant, computed and detached keys ---
console.log(Symbol.keyFor(Symbol.for("@@__perry_wk_iterator")));
const constKey = "@@__perry_wk_iterator";
console.log(Symbol.keyFor(Symbol.for(constKey)));
console.log(keyFor("@@__perry_wk_" + ["itera", "tor"].join("")));
console.log(keyFor("@@__perry_wk_iterator"));
const symbolFor = Symbol.for;
console.log(Symbol.keyFor(symbolFor("@@__perry_wk_iterator")));
console.log(keyFor("@@__perry_wk_"));
console.log(keyFor("@@__perry_wk_noSuchName"));

const registered = Symbol.for("@@__perry_wk_iterator");
console.log(registered === Symbol.for(constKey), registered === Symbol.for("@@__perry_wk_iter" + "ator"));
console.log((registered as symbol) === Symbol.iterator);
console.log(registered.description, String(registered), typeof registered);

// --- well-known symbols stay unregistered ---
for (const wk of [
  Symbol.iterator,
  Symbol.asyncIterator,
  Symbol.hasInstance,
  Symbol.toPrimitive,
  Symbol.toStringTag,
  Symbol.species,
  Symbol.isConcatSpreadable,
  Symbol.match,
  Symbol.matchAll,
  Symbol.replace,
  Symbol.search,
  Symbol.split,
  Symbol.unscopables,
  Symbol["iterator"],
]) {
  console.log(String(wk), wk.description, Symbol.keyFor(wk));
}

// --- as property keys ---
const o: any = { [Symbol.for("@@__perry_wk_iterator")]: "registered" };
console.log(o[Symbol.iterator], o[Symbol.for("@@__perry_wk_iterator")], Symbol.iterator in o);
console.log([].concat({ [Symbol.for("@@__perry_wk_isConcatSpreadable")]: true, length: 1, 0: "x" } as any).length);
console.log(Object.prototype.toString.call({ [Symbol.for("@@__perry_wk_toStringTag")]: "Tag" }));

const a: any = [1, 2];
console.log(a[Symbol.for("@@__perry_wk_iterator")], typeof a[Symbol.iterator]);
const u8: any = new Uint8Array(2);
console.log(u8[Symbol.for("@@__perry_wk_toStringTag")], u8[Symbol.toStringTag]);

// --- as class member keys ---
class Plain {
  [Symbol.for("@@__perry_wk_iterator")]() { return "registered"; }
  [Symbol.for("@@__perry_wk_toPrimitive")]() { return 7; }
  static [Symbol.for("@@__perry_wk_hasInstance")]() { return true; }
}
const p: any = new Plain();
console.log(typeof p[Symbol.iterator], p[Symbol.for("@@__perry_wk_iterator")]());
console.log(`${p}`, ({} as any) instanceof Plain);
try {
  console.log([...p]);
} catch (e) {
  console.log(e instanceof TypeError);
}

class Both {
  *[Symbol.iterator]() { yield 1; yield 2; }
  [Symbol.for("@@__perry_wk_iterator")]() { return "registered"; }
}
const b: any = new Both();
console.log([...b], b[Symbol.for("@@__perry_wk_iterator")]());
console.log(Object.getOwnPropertySymbols(Both.prototype).map(String).sort());
