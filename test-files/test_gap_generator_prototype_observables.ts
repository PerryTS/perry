// A sync generator instance's [[Prototype]] is its function's `prototype`
// at the call (GetPrototypeFromConstructor), held per instance. Every order
// of first observation, reassignment and mutation must agree with node:
// Object.getPrototypeOf / Reflect / __proto__, identity across instances,
// reassignment before and after a birth, instanceof, isPrototypeOf, methods
// added to `g.prototype` after the instance exists, and the declared
// `prototype` property of the function itself.
const log: string[] = [];
const out = (s: string) => log.push(s);
const GenProto = Object.getPrototypeOf(function* () {}.prototype);

// 1. observe after the birth, then before a later birth
function* g1() {
  yield 1;
}
const a1 = g1();
out("1 after-birth " + (Object.getPrototypeOf(a1) === g1.prototype));
out("1 later-birth " + (Object.getPrototypeOf(g1()) === g1.prototype));
out("1 stable " + (g1.prototype === g1.prototype));
out("1 chain " + (Object.getPrototypeOf(g1.prototype) === GenProto));
out("1 own-names " + JSON.stringify(Object.getOwnPropertyNames(g1.prototype)));

// 2. observe the function's prototype first
function* g2() {
  yield 2;
}
const p2 = g2.prototype;
out("2 before-birth " + (Object.getPrototypeOf(g2()) === p2));

// 2b. a computed-key read, Reflect.get and `in` see the declared slot
function* g2b() {
  yield 2;
}
const a2b = g2b();
const key2b = ["proto", "type"].join("");
const viaKey = (g2b as any)[key2b];
out("2b key " + (Object.getPrototypeOf(a2b) === viaKey));
function* g2c() {
  yield 2;
}
const a2c = g2c();
const viaReflect = Reflect.get(g2c, "prototype");
out("2b reflect " + (Object.getPrototypeOf(a2c) === viaReflect) + " " + ("prototype" in g2c));

// 3. two instances born unobserved share one original
function* g3() {
  yield 3;
}
const a3 = g3();
const b3 = g3();
out("3 shared " + (Object.getPrototypeOf(a3) === Object.getPrototypeOf(b3)));
out("3 is-fn-proto " + (Object.getPrototypeOf(b3) === g3.prototype));

// 4. reassignment before a birth
function* g4() {
  yield 4;
}
const x4 = { tag: "x4" };
g4.prototype = x4 as any;
out("4 reassigned " + (Object.getPrototypeOf(g4()) === x4));
out("4 tag " + (g4() as any).tag);

// 5. reassignment after an unobserved birth keeps the original
function* g5() {
  yield 5;
}
const a5 = g5();
const b5 = g5();
const x5 = { tag: "x5" };
g5.prototype = x5 as any;
const c5 = g5();
const o5 = Object.getPrototypeOf(a5);
out("5 old-not-new " + (o5 !== x5));
out("5 old-shared " + (o5 === Object.getPrototypeOf(b5)));
out("5 old-chain " + (Object.getPrototypeOf(o5) === GenProto));
out("5 old-empty " + JSON.stringify(Object.getOwnPropertyNames(o5)));
out("5 new " + (Object.getPrototypeOf(c5) === x5));
out("5 old-tag " + (a5 as any).tag);
out("5 old-instanceof " + (a5 instanceof (g5 as any)));
out("5 new-instanceof " + (c5 instanceof (g5 as any)));

// 6. reassignment after a birth whose prototype was observed
function* g6() {
  yield 6;
}
const a6 = g6();
const o6 = g6.prototype;
g6.prototype = {} as any;
out("6 observed-original " + (Object.getPrototypeOf(a6) === o6));

// 7. a primitive prototype selects %GeneratorPrototype%
function* g7() {
  yield 7;
}
(g7 as any).prototype = 5;
out("7 primitive " + (Object.getPrototypeOf(g7()) === GenProto));
(g7 as any).prototype = null;
out("7 null " + (Object.getPrototypeOf(g7()) === GenProto));

// 8. instanceof and isPrototypeOf
function* g8() {
  yield 8;
}
const a8 = g8();
out("8 instanceof " + (a8 instanceof (g8 as any)));
out("8 isPrototypeOf-fn " + g8.prototype.isPrototypeOf(a8));
out("8 isPrototypeOf-gen " + GenProto.isPrototypeOf(a8));
out("8 isPrototypeOf-obj " + Object.prototype.isPrototypeOf(a8));
out("8 not-other " + (a8 instanceof (g1 as any)));

// 9. methods added after the instance exists
function* g9() {
  yield 9;
  yield 10;
}
const a9 = g9();
(g9.prototype as any).extra = function (this: any) {
  return "extra:" + (this === a9);
};
out("9 extra " + (a9 as any).extra());
out("9 in " + ("extra" in a9) + " " + ("next" in a9) + " " + a9.hasOwnProperty("next"));
const b9 = g9();
(g9.prototype as any).next = function () {
  return { value: "patched", done: true };
};
out("9 patched-next " + JSON.stringify(b9.next()));
const seen9: unknown[] = [];
for (const v of g9()) seen9.push(v);
out("9 patched-forof " + JSON.stringify(seen9));
delete (g9.prototype as any).next;
out("9 restored " + JSON.stringify([...g9()]));

// 10. methods added to %GeneratorPrototype% reach every instance
function* g10() {
  yield 1;
}
const a10 = g10();
(GenProto as any).viaGen = function () {
  return "viaGen";
};
out("10 gen-proto " + (a10 as any).viaGen() + " " + (g1() as any).viaGen());
delete (GenProto as any).viaGen;
out("10 removed " + typeof (a10 as any).viaGen);

// 11. Reflect, __proto__, setPrototypeOf
function* g11() {
  yield 11;
}
const a11 = g11();
out("11 reflect " + (Reflect.getPrototypeOf(a11) === g11.prototype));
const b11 = g11();
out("11 __proto__ " + ((b11 as any).__proto__ === g11.prototype));
const c11 = g11();
Object.setPrototypeOf(c11, GenProto);
out("11 set-to-gen " + (Object.getPrototypeOf(c11) === GenProto) + " " + JSON.stringify(c11.next()));
const d11 = g11();
const x11 = {
  next() {
    return { value: "own", done: true };
  },
};
Object.setPrototypeOf(d11, x11);
out("11 set-to-obj " + (Object.getPrototypeOf(d11) === x11) + " " + JSON.stringify(d11.next()));
const e11 = g11();
out("11 set-same " + Reflect.setPrototypeOf(e11, g11.prototype) + " " + (Object.getPrototypeOf(e11) === g11.prototype));

// 12. the function's declared `prototype` property
function* g12() {
  yield 12;
}
out("12 names " + JSON.stringify(Object.getOwnPropertyNames(g12)));
out("12 desc " + JSON.stringify(Object.getOwnPropertyDescriptor(g12, "prototype"), (k, v) => (k === "value" ? typeof v : v)));
out("12 delete " + Reflect.deleteProperty(g12, "prototype") + " " + typeof g12.prototype);
function* g12b() {
  yield 12;
}
(g12b as any).foo = 1;
out("12 order " + JSON.stringify(Object.getOwnPropertyNames(g12b)));
out("12b proto " + (Object.getPrototypeOf(g12b()) === g12b.prototype));
function* g12c() {
  yield 12;
}
const x12 = {};
Object.defineProperty(g12c, "prototype", { value: x12 });
out("12c define " + (Object.getPrototypeOf(g12c()) === x12));
out("12c desc " + JSON.stringify(Object.getOwnPropertyDescriptor(g12c, "prototype"), (k, v) => (k === "value" ? typeof v : v)));
out("12 keys " + JSON.stringify(Object.keys(g12)) + " " + g12.hasOwnProperty("prototype"));

// 13. fresh closures of one body: distinct prototypes, each instance its own
function make(i: number) {
  return function* () {
    yield i;
  };
}
const f13a = make(1);
const f13b = make(2);
const i13a = f13a();
const i13b = f13b();
out("13 distinct " + (f13a.prototype !== f13b.prototype));
out("13 own-a " + (Object.getPrototypeOf(i13a) === f13a.prototype));
out("13 own-b " + (Object.getPrototypeOf(i13b) === f13b.prototype));
out("13 not-cross " + (Object.getPrototypeOf(i13a) !== f13b.prototype));
out("13 values " + JSON.stringify([...i13a, ...i13b]));

// 14. methods: object literal and class
const o14 = {
  *m() {
    yield 14;
  },
};
out("14 literal " + (Object.getPrototypeOf(o14.m()) === o14.m.prototype));
class C14 {
  *values() {
    yield 1;
  }
  static *s() {
    yield 2;
  }
}
const c14 = new C14();
out("14 class " + (Object.getPrototypeOf(c14.values()) === C14.prototype.values.prototype));
out("14 static " + (Object.getPrototypeOf(C14.s()) === C14.s.prototype));

// 15. own properties, integrity, reflection on instances
function* g15() {
  yield 15;
}
const a15 = g15() as any;
a15.x = 1;
out("15 own " + JSON.stringify(Object.keys(a15)) + " " + (Object.getPrototypeOf(a15) === g15.prototype));
const b15 = Object.freeze(g15());
out("15 frozen " + JSON.stringify(b15.next()) + " " + Object.isFrozen(b15) + " " + (Object.getPrototypeOf(b15) === g15.prototype));
out("15 tag " + Object.prototype.toString.call(g15()) + " " + String(g15()));
out("15 json " + JSON.stringify(g15()) + " " + JSON.stringify({ ...g15() }));
out("15 names " + JSON.stringify(Object.getOwnPropertyNames(g15())) + " " + Object.getOwnPropertySymbols(g15()).length);
const p15 = new Proxy(g15(), {});
out("15 proxy " + (Object.getPrototypeOf(p15) === g15.prototype));
const k15 = Object.create(Object.getPrototypeOf(g15()));
out("15 create " + (Object.getPrototypeOf(k15) === g15.prototype));
out("15 iterator " + (g15()[Symbol.iterator]() !== undefined));

// 17. prototypes that are not ordinary objects: a function (even the
// generator function itself), a Proxy, an array, another function's original
function* g17() {
  yield 17;
}
const f17 = function () {};
(f17 as any).marker = "fn";
g17.prototype = f17 as any;
const a17 = g17() as any;
out("17 fn " + (Object.getPrototypeOf(a17) === f17) + " " + a17.marker + " " + typeof a17.next);
const px17 = new Proxy({ marker: "px" }, {});
g17.prototype = px17 as any;
const b17 = g17() as any;
out("17 proxy " + (Object.getPrototypeOf(b17) === px17) + " " + b17.marker);
const arr17 = [7, 8];
g17.prototype = arr17 as any;
const c17 = g17() as any;
out("17 array " + (Object.getPrototypeOf(c17) === arr17) + " " + c17.length + " " + c17[1]);
function* g17s() {
  yield 1;
}
(g17s as any).prototype = g17s;
const d17 = g17s();
out("17 self " + (Object.getPrototypeOf(d17) === g17s) + " " + typeof (d17 as any).next);
function* g17o() {
  yield 1;
}
g17o.prototype = g1.prototype;
const e17 = g17o();
out("17 other-original " + (Object.getPrototypeOf(e17) === g1.prototype) + " " + (e17 instanceof (g1 as any)));

// 16. a thenable prototype is observed by await
function* g16() {
  yield 16;
}
async function awaits() {
  const plain = await g16();
  out("16 plain " + (Object.getPrototypeOf(plain) === g16.prototype));
  const a16 = g16();
  (g16.prototype as any).then = function (resolve: (v: unknown) => void) {
    resolve("thenable");
  };
  out("16 thenable " + (await a16));
  delete (g16.prototype as any).then;
}

awaits().then(() => {
  for (const line of log) console.log(line);
});
