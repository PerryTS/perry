// Stage 0 of #10868 step 2.5 (canonical shape identity).
//
// Key ORDER is observable in JS, so a canonicalisation that merged two layouts
// with the same key SET but different order — or a deleted-then-re-added key
// with a never-deleted one — is a silent wrong answer in every program, not a
// slow one. These rows pin the order across every way an object can reach a
// layout: born from a literal, grown by name, grown past its inline region
// into spill, tombstoned and re-added, and reached through defineProperty.
//
// Every row prints. Byte-identical to node is the contract.
//
// PROVEN ABLE TO FAIL — each view was reddened by a sabotage applied to the
// runtime and reverted, because a row that cannot redden is documentation.
// The four views reach the key list by THREE different paths, which is why
// one sabotage was not enough:
//
//   view        path                                        sabotage that reddens it
//   ----        ----                                        ------------------------
//   keys=       js_object_keys (field_get_set/enumeration)  reverse js_object_keys' result: 37 lines
//   forin=      same function as keys=                      (same sabotage; keys= and forin= move together)
//   own=        js_object_get_own_property_names            reverse its result: own= only, 19 rows
//               (object/descriptors.rs, via
//               Reflect.ownKeys in proxy/reflect_misc.rs)
//   json=       object_keys_array read DIRECTLY by the      reverse the keys array at publication
//               json/stringify_* serialisers — it bypasses  (object/mod.rs, before
//               both enumeration functions above            publish_object_shape_from): json= 9 rows,
//                                                           and keys/forin/own too
//
// The row that matters most is `ab` / `ba`: under the first sabotage both come
// back in the same order, i.e. the sabotage merges the two layouts this file
// exists to keep apart. Under the third, `hole` prints {"d":1,"b":3,"a":4} —
// keys reversed, values left in their slots — which is the silent-wrong-value
// failure a wrong canonicalisation would produce.

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
const ab = { a: 1, b: 2 };
const ba = { b: 2, a: 1 };
show("ab", ab);
show("ba", ba);

// --- 3. tombstone: delete then re-add moves the key to the END --------------
const tomb = { a: 1, b: 2, c: 3 };
delete tomb.a;
tomb.a = 9;
show("tomb", tomb);
// ... and must not equal an object built directly in that order with the same
// values, as far as ORDER is concerned (they do match here, which is the
// point: the ORDER is what is observable, not the history).
const direct = { b: 2, c: 3, a: 9 };
show("direct", direct);

// --- 4. delete in the middle, no re-add -------------------------------------
const hole = { a: 1, b: 2, c: 3, d: 4 };
delete hole.b;
show("hole", hole);
const holeGrown = {};
holeGrown.a = 1;
holeGrown.b = 2;
holeGrown.c = 3;
holeGrown.d = 4;
delete holeGrown.b;
show("holeGrown", holeGrown);

// --- 5. past the inline region: born wide vs grown wide ---------------------
// INLINE_SLOT_FLOOR is 2, so a grown object spills from its third key while a
// literal is born with exactly its own count. Same layout, different physical
// placement — the pair L8.3.7 exists for.
const bornWide = { k0: 0, k1: 1, k2: 2, k3: 3, k4: 4, k5: 5, k6: 6, k7: 7 };
const grownWide = {};
for (let i = 0; i < 8; i++) grownWide["k" + i] = i;
show("bornWide", bornWide);
show("grownWide", grownWide);

// --- 6. integer-like keys sort before string keys, ascending ----------------
const mixed = {};
mixed.z = 1;
mixed[2] = 2;
mixed.a = 3;
mixed[10] = 4;
mixed[1] = 5;
show("mixed", mixed);
const mixedBorn = { z: 1, 2: 2, a: 3, 10: 4, 1: 5 };
show("mixedBorn", mixedBorn);

// --- 7. defineProperty: enumerable false is in ownKeys, not in keys ---------
const dp = { a: 1 };
Object.defineProperty(dp, "hidden", { value: 2, enumerable: false, configurable: true });
dp.b = 3;
show("dp", dp);
const dpGrown = {};
dpGrown.a = 1;
Object.defineProperty(dpGrown, "hidden", { value: 2, enumerable: false, configurable: true });
dpGrown.b = 3;
show("dpGrown", dpGrown);

// --- 8. accessor keeps its insertion position -------------------------------
const acc = { a: 1 };
Object.defineProperty(acc, "g", { get: function () { return 7; }, enumerable: true, configurable: true });
acc.b = 3;
show("acc", acc);
console.log("acc.g=" + acc.g);

// --- 9. symbols come after strings in ownKeys -------------------------------
const sym = { a: 1 };
const S = Symbol("s");
sym[S] = 2;
sym.b = 3;
show("sym", sym);

// --- 10. null-prototype objects -------------------------------------------
const np = Object.create(null);
np.a = 1;
np.b = 2;
console.log("np keys=" + JSON.stringify(Object.keys(np)));
console.log("np own=" + JSON.stringify(Reflect.ownKeys(np)));

// --- 11. two literal SITES with equal key lists -----------------------------
// Different static keys arrays today; one layout under a content key. Order
// must be identical either way.
function siteA() { return { p: 1, q: 2 }; }
function siteB() { return { p: 3, q: 4 }; }
show("siteA", siteA());
show("siteB", siteB());
