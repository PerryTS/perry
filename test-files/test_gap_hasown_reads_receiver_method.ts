// `x.hasOwnProperty(k)` and `x.propertyIsEnumerable(k)` are ordinary method
// calls: the method is read off the receiver, so an own method, a class
// method, or a replaced `Object.prototype` method is what runs. A lowering
// that turns the call into the builtin by name answers `true`/`false` here
// instead of the method's own result.

const out: any[] = [];
function row(label: string, f: () => any) {
  try {
    out.push([label, f()]);
  } catch (e: any) {
    out.push([label, "threw", e instanceof TypeError]);
  }
}

// --- the receiver's own method wins -----------------------------------------
row("own hop on literal", () => ({ hasOwnProperty(_k: string) { return "own"; } } as any).hasOwnProperty("x"));
row("own pie on literal", () => ({ propertyIsEnumerable(_k: string) { return "own-pie"; } } as any).propertyIsEnumerable("x"));
row("own hop data property", () => ({ hasOwnProperty: (k: string) => "arrow:" + k } as any).hasOwnProperty("y"));

class WithHop {
  a = 1;
  hasOwnProperty(k: string) { return "class:" + k; }
  propertyIsEnumerable(k: string) { return "class-pie:" + k; }
}
const wh: any = new WithHop();
row("class hop", () => wh.hasOwnProperty("a"));
row("class pie", () => wh.propertyIsEnumerable("a"));
class SubHop extends WithHop {}
row("subclass hop", () => (new SubHop() as any).hasOwnProperty("a"));

const np: any = Object.create(null);
np.k = 1;
np.hasOwnProperty = function (k: string) { return "nullproto:" + k; };
np.propertyIsEnumerable = function (k: string) { return "nullproto-pie:" + k; };
row("null-proto own hop", () => np.hasOwnProperty("k"));
row("null-proto own pie", () => np.propertyIsEnumerable("k"));
const bare: any = Object.create(null);
bare.k = 1;
row("null-proto no hop", () => bare.hasOwnProperty("k"));
row("null-proto no pie", () => bare.propertyIsEnumerable("k"));

// --- a replaced Object.prototype method is what plain objects call ----------
const savedHop = Object.prototype.hasOwnProperty;
const savedPie = Object.prototype.propertyIsEnumerable;
(Object.prototype as any).hasOwnProperty = function (k: string) { return "replaced:" + k; };
(Object.prototype as any).propertyIsEnumerable = function (k: string) { return "replaced-pie:" + k; };
row("replaced hop", () => ({} as any).hasOwnProperty("k"));
row("replaced pie", () => ({ k: 1 } as any).propertyIsEnumerable("k"));
class Plain { x = 1; }
row("replaced hop on class instance", () => (new Plain() as any).hasOwnProperty("x"));
function viaParam(o: any, k: string) { return o.hasOwnProperty(k); }
let last: any = null;
for (let i = 0; i < 20; i++) last = viaParam({ k: i }, "k");
row("replaced hop through param", () => last);
(Object.prototype as any).hasOwnProperty = savedHop;
(Object.prototype as any).propertyIsEnumerable = savedPie;
row("restored hop", () => ({ k: 1 } as any).hasOwnProperty("k"));
row("restored pie", () => ({ k: 1 } as any).propertyIsEnumerable("k"));
for (let i = 0; i < 20; i++) last = viaParam({ k: i }, "k");
row("restored hop through param", () => last);

// --- plain semantics ---------------------------------------------------------
const proto: any = { inherited: 1 };
const o: any = Object.create(proto);
o.own = 2;
const sym = Symbol("s");
const absentSym = Symbol("absent");
o[sym] = 3;
o[7] = "seven";
Object.defineProperty(o, "hidden", { value: 4, enumerable: false });
row("own key", () => o.hasOwnProperty("own"));
row("missing key", () => o.hasOwnProperty("nope"));
row("inherited key", () => o.hasOwnProperty("inherited"));
row("own symbol", () => o.hasOwnProperty(sym));
row("absent symbol", () => o.hasOwnProperty(absentSym));
row("numeric key as number", () => o.hasOwnProperty(7));
row("numeric key as string", () => o.hasOwnProperty("7"));
row("non-enumerable own hop", () => o.hasOwnProperty("hidden"));
row("pie own", () => o.propertyIsEnumerable("own"));
row("pie inherited", () => o.propertyIsEnumerable("inherited"));
row("pie non-enumerable", () => o.propertyIsEnumerable("hidden"));
row("pie own symbol", () => o.propertyIsEnumerable(sym));
row("pie numeric", () => o.propertyIsEnumerable(7));

const arr: any = [10, 20, 30];
row("array index", () => arr.hasOwnProperty(0));
row("array index string", () => arr.hasOwnProperty("2"));
row("array past end", () => arr.hasOwnProperty(3));
row("array length", () => arr.hasOwnProperty("length"));
row("array method", () => arr.hasOwnProperty("push"));
row("array pie index", () => arr.propertyIsEnumerable(1));
row("array pie length", () => arr.propertyIsEnumerable("length"));

row("string length via call", () => Object.prototype.hasOwnProperty.call("abc", "length"));
row("string index via call", () => Object.prototype.hasOwnProperty.call("abc", 1));
row("string past end via call", () => Object.prototype.hasOwnProperty.call("abc", 5));
row("string pie index via call", () => Object.prototype.propertyIsEnumerable.call("abc", 0));
row("string pie length via call", () => Object.prototype.propertyIsEnumerable.call("abc", "length"));
row("string direct index", () => ("abc" as any).hasOwnProperty("0"));
row("string direct method", () => ("abc" as any).hasOwnProperty("charAt"));
row("number direct", () => (5 as any).hasOwnProperty("x"));
row("boolean direct", () => (true as any).propertyIsEnumerable("x"));

// --- builtin receivers reach the real methods --------------------------------
row("regexp lastIndex", () => (/x/g as any).hasOwnProperty("lastIndex"));
row("RegExp.prototype pie global", () => (RegExp.prototype as any).propertyIsEnumerable("global"));
row("RegExp.prototype hop exec", () => RegExp.prototype.hasOwnProperty("exec"));
row("Date.prototype hop getTime", () => Date.prototype.hasOwnProperty("getTime"));
row("Date.prototype pie getTime", () => Date.prototype.propertyIsEnumerable("getTime"));
row("map instance size", () => (new Map() as any).hasOwnProperty("size"));
row("Map.prototype hop get", () => Map.prototype.hasOwnProperty("get"));
row("Number.prototype pie toFixed", () => Number.prototype.propertyIsEnumerable("toFixed"));
row("String.prototype hop charAt", () => String.prototype.hasOwnProperty("charAt"));
function named() {}
row("function name", () => (named as any).hasOwnProperty("name"));
row("function pie name", () => (named as any).propertyIsEnumerable("name"));
row("error message", () => new Error("m").hasOwnProperty("message"));
row("error pie message", () => new Error("m").propertyIsEnumerable("message"));
const errWithProps: any = new Error("m", { cause: 1 });
errWithProps.extra = 1;
row("error pie user prop", () => errWithProps.propertyIsEnumerable("extra"));
row("error pie cause", () => errWithProps.propertyIsEnumerable("cause"));
row("error hop cause", () => errWithProps.hasOwnProperty("cause"));
row("error pie stack", () => errWithProps.propertyIsEnumerable("stack"));
row("set instance", () => (new Set([1]) as any).propertyIsEnumerable(1));
row("symbol receiver", () => (Symbol("q") as any).hasOwnProperty("description"));

// --- reading the method off a builtin receiver gives a callable --------------
row("reflective hop from Map.prototype", () => {
  const f = (Map.prototype as any).hasOwnProperty;
  return [typeof f, f.call({ z: 1 }, "z"), f.call({ z: 1 }, "w")];
});
row("reflective pie from RegExp.prototype", () => {
  const f = (RegExp.prototype as any).propertyIsEnumerable;
  return [typeof f, f.call({ z: 1 }, "z"), f.call([1], "length")];
});

for (const r of out) console.log(JSON.stringify(r));
