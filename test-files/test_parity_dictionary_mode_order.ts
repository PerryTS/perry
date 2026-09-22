// Dictionary mode (#10868 step 2.5 stage 1) — the ORDER contract.
//
// A dictionary-mode object stops pointing at a layout the shape table owns and
// carries its own ordered key list in its `ObjectMeta`. Key order is
// observable in JS, so if the private list reproduces the order differently
// from the shared one — or if a consumer of the shape's key list was never
// branched — the result is a silent wrong answer in every program, not a slow
// one. These rows pin the order across every way an object can reach a
// layout, exactly as `test_parity_shape_identity_order.ts` does for an
// ordinary object.
//
// RUN IT TWICE. This file is byte-identical to node with the latch off (the
// default) AND with `PERRY_OBJECT_DICTIONARY_MIN_KEYS=0` set at COMPILE time,
// which forces every receiver that publishes a key list into dictionary mode.
// The second run is the point: it puts the whole file through the mode, so a
// consumer of the shape's key list that nobody branched shows up here as a
// diff rather than as a plausible-looking pass.
//
// PROVEN ABLE TO FAIL — each view was reddened by a sabotage applied to the
// runtime and reverted, because a row that cannot redden is documentation.
// The four views reach the key list by THREE different paths, which is why
// one sabotage is not enough. The sabotages are the dictionary-mode twins of
// the three in #10919, and each names the enforcement point it removes:
//
//   view    path                                     sabotage that reddens it
//   ----    ----                                     ------------------------
//   keys=   js_object_keys -> object_keys_array      make `object/mod.rs`'s
//   forin=  (same function; the two move together)   dictionary branch return the
//                                                    shape's null keys instead of
//                                                    `dictionary::keys_array` —
//                                                    every row loses every key
//   own=    js_object_get_own_property_names,        reverse `clone_key_list`'s copy
//           a DIFFERENT walk of the same array       in `object/dictionary.rs`: the
//                                                    latch is where the private list
//                                                    is built, so a wrong order there
//                                                    is invisible until an object
//                                                    latches
//   json=   object_keys_array read DIRECTLY by the   skip `publish_keys`' store when
//           json/stringify_* serialisers, bypassing  the array reallocates: appends
//           both enumeration functions above         past the latch's slack vanish
//
// The row that matters most is `grownWide`: it grows past the inline region so
// its values live in spill, which is the case where "the names moved but the
// values did not" has to hold or the printed values are silently wrong.

function show(label, o) {
  const keys = Object.keys(o);
  const forin = [];
  for (const k in o) forin.push(k);
  const own = Reflect.ownKeys(o).map((k) => (typeof k === "symbol" ? String(k) : k));
  console.log(label + " keys=" + JSON.stringify(keys));
  console.log(label + " forin=" + JSON.stringify(forin));
  console.log(label + " own=" + JSON.stringify(own));
  console.log(label + " json=" + JSON.stringify(o));
}

// --- 1. born vs grown, same key set, same order -----------------------------
const born3 = { a: 1, b: 2, c: 3 };
const grown3 = {};
grown3.a = 1;
grown3.b = 2;
grown3.c = 3;
show("born3", born3);
show("grown3", grown3);

// --- 2. same SET, different ORDER. These must NOT be merged -----------------
const ab = {};
ab.a = 1;
ab.b = 2;
const ba = {};
ba.b = 2;
ba.a = 1;
show("ab", ab);
show("ba", ba);

// --- 3. tombstone: delete then re-add moves the key to the END --------------
const tomb = {};
tomb.a = 1;
tomb.b = 2;
tomb.c = 3;
delete tomb.a;
tomb.a = 9;
show("tomb", tomb);
const direct = {};
direct.b = 2;
direct.c = 3;
direct.a = 9;
show("direct", direct);

// --- 4. delete in the middle, no re-add -------------------------------------
const hole = {};
hole.a = 1;
hole.b = 2;
hole.c = 3;
hole.d = 4;
delete hole.b;
show("hole", hole);

// --- 5. WIDE: grown past the inline region, so the values live in spill -----
// This is the row dictionary mode is built for: a key list unique to one
// object, grown by name. The names move to the meta record; the values must
// not move at all.
const grownWide = {};
for (let i = 0; i < 24; i++) grownWide["w" + i] = i * 3;
show("grownWide", grownWide);
const bornWide = { w0: 0, w1: 3, w2: 6, w3: 9, w4: 12, w5: 15, w6: 18, w7: 21 };
show("bornWide", bornWide);

// --- 6. integer-like keys sort FIRST, ascending, then strings ---------------
const mixed = {};
mixed.z = 1;
mixed["2"] = 2;
mixed.a = 3;
mixed["10"] = 4;
mixed["1"] = 5;
show("mixed", mixed);

// --- 7. a non-enumerable key is in ownKeys, not in keys ---------------------
const dp = {};
dp.a = 1;
Object.defineProperty(dp, "hidden", { value: 2, enumerable: false, configurable: true });
dp.b = 3;
show("dp", dp);

// --- 8. an accessor keeps its insertion position ----------------------------
const acc = {};
acc.a = 1;
Object.defineProperty(acc, "g", { get() { return 7; }, enumerable: true, configurable: true });
acc.b = 3;
show("acc", acc);
console.log("acc g=" + acc.g);

// --- 9. symbols come after strings in ownKeys -------------------------------
const sym = {};
sym.a = 1;
sym[Symbol.for("s")] = 2;
sym.b = 3;
show("sym", sym);

// --- 10. a null-prototype object --------------------------------------------
const np = Object.create(null);
np.a = 1;
np.b = 2;
show("np", np);

// --- 11. an own field must still shadow a prototype method ------------------
// The own-field shadowing scan reads the shape's key list; on a dictionary
// object that list is empty, so an unbranched scan lets the vtable method win.
const shadow = {};
shadow.a = 1;
shadow.toString = function () { return "own-toString"; };
console.log("shadow=" + String(shadow));
console.log("shadow keys=" + JSON.stringify(Object.keys(shadow)));

// --- 12. an own property must not be answered from the prototype chain ------
const protoOwner = { p: "from-proto" };
const child = Object.create(protoOwner);
for (let i = 0; i < 12; i++) child["c" + i] = i;
child.p = "own";
console.log("child p=" + child.p);
console.log("child hasOwn p=" + Object.prototype.hasOwnProperty.call(child, "p"));
console.log("child in p=" + ("p" in child));
show("child", child);

// --- 13. read-back after the whole lot --------------------------------------
let sum = 0;
for (const k in grownWide) sum += grownWide[k];
console.log("grownWide sum=" + sum);
console.log("grownWide w23=" + grownWide.w23);
console.log("grownWide delete=" + delete grownWide.w5);
console.log("grownWide after delete=" + JSON.stringify(Object.keys(grownWide)));
grownWide.w5 = 999;
console.log("grownWide readd=" + JSON.stringify(Object.keys(grownWide)));
console.log("grownWide w5=" + grownWide.w5);
