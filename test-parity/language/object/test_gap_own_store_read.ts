// A computed read after a store of the same object and key reads what the
// store made own (three.js `cloneUniforms`), never the Object.prototype
// members, also under keys that name them ("constructor", "toString").
"use strict";
class V {
  n: number;
  constructor(n: number) {
    this.n = n;
  }
  clone(): V {
    return new V(this.n + 1);
  }
  size(): number {
    return this.n;
  }
}

function cloneUniforms(src: any): any {
  const dst: any = {};
  for (const u in src) {
    dst[u] = {};
    for (const p in src[u]) {
      const property = src[u][p];
      if (property instanceof V) {
        dst[u][p] = property.clone();
      } else if (Array.isArray(property)) {
        const arr: V[] = [];
        for (let i = 0; i < property.length; i++) arr.push(property[i].clone());
        dst[u][p] = arr;
      } else {
        dst[u][p] = property;
      }
    }
  }
  return dst;
}

function make(names: string[]): any {
  const o: any = {};
  for (const n of names) o[n] = { value: new V(names.indexOf(n)), list: [new V(10), new V(20)], tag: n };
  return o;
}

const out = cloneUniforms(make(["a", "constructor", "toString", "valueOf", "hasOwnProperty"]));
for (const k of Object.keys(out)) {
  const u = out[k];
  console.log(k, u.value.size(), u.list.length, u.list[1].size(), u.tag);
}
console.log(out.constructor.value.size(), typeof out.toString, Object.getPrototypeOf(out) === Object.prototype);

// The same key binding under a constant `__proto__`: the store sets the
// prototype and the read returns it.
function protoKey(v: V): any {
  const o: any = {};
  const k = "__proto__";
  o[k] = v;
  const p = o[k];
  return p;
}
const pk = protoKey(new V(7));
console.log(pk.size(), pk instanceof V);

function protoMaybe(v: V, which: number): any {
  const o: any = {};
  const k = which > 99 ? "x" : "__proto__";
  o[k] = v;
  const p = o[k];
  return p;
}
console.log(protoMaybe(new V(8), process.argv.length).size());

// A store under a different binding, or a read before the store, keeps
// the chain.
function before(src: any): string[] {
  const o: any = {};
  const seen: string[] = [];
  for (const k in src) {
    seen.push(typeof o[k]);
    o[k] = 1;
  }
  return seen;
}
console.log(before({ a: 1, constructor: 2, toString: 3 }).join(","));
