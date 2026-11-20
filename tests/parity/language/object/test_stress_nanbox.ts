// Stress test: NaN-boxing value representation
// Targets: every value type roundtrips correctly through storage/retrieval
// Based on bugs: TAG_UNDEFINED vs 0.0, Buffer byte access, fetch segfault,
// FsReadFileBinary NaN-boxing, alloca init to TAG_UNDEFINED

// === SECTION: typeof for every value type ===
const undef = undefined;
const nul = null;
const t = true;
const f = false;
const zero = 0;
const negZero = -0;
const nan = NaN;
const inf = Infinity;
const negInf = -Infinity;
const int = 42;
const float = 3.14159;
const str = "hello";
const arr: number[] = [1, 2, 3];
const obj = { x: 1 };

console.log("typeof undefined:", typeof undef);
console.log("typeof null:", typeof nul);
console.log("typeof true:", typeof t);
console.log("typeof false:", typeof f);
console.log("typeof 0:", typeof zero);
console.log("typeof -0:", typeof negZero);
console.log("typeof NaN:", typeof nan);
console.log("typeof Infinity:", typeof inf);
console.log("typeof -Infinity:", typeof negInf);
console.log("typeof 42:", typeof int);
console.log("typeof 3.14:", typeof float);
console.log("typeof string:", typeof str);
console.log("typeof array:", typeof arr);
console.log("typeof object:", typeof obj);

// === SECTION: Values stored in arrays ===
const values: any[] = [undefined, null, true, false, 0, -0, NaN, Infinity, -Infinity, 42, 3.14, "hello", [1, 2], { x: 1 }];
console.log("arr[0] undefined:", values[0] === undefined);
console.log("arr[1] null:", values[1] === null);
console.log("arr[2] true:", values[2] === true);
console.log("arr[3] false:", values[3] === false);
console.log("arr[4] zero:", values[4] === 0);
console.log("arr[6] NaN:", Number.isNaN(values[6]));
console.log("arr[7] Infinity:", values[7] === Infinity);
console.log("arr[8] -Infinity:", values[8] === -Infinity);
console.log("arr[9] 42:", values[9] === 42);
console.log("arr[11] hello:", values[11] === "hello");

// === SECTION: Values stored in object fields ===
const box: any = {};
box.undef = undefined;
box.nul = null;
box.t = true;
box.f = false;
box.zero = 0;
box.nan = NaN;
box.inf = Infinity;
box.negInf = -Infinity;
box.num = 42;
box.flt = 3.14;
box.str = "world";
box.arr = [1, 2, 3];
box.obj = { y: 2 };

console.log("obj.undef:", box.undef);
console.log("obj.nul:", box.nul);
console.log("obj.t:", box.t);
console.log("obj.f:", box.f);
console.log("obj.zero:", box.zero);
console.log("obj.nan:", Number.isNaN(box.nan));
console.log("obj.inf:", box.inf);
console.log("obj.negInf:", box.negInf);
console.log("obj.num:", box.num);
console.log("obj.flt:", box.flt);
console.log("obj.str:", box.str);
console.log("obj.arr:", box.arr);
console.log("obj.obj.y:", box.obj.y);

// === SECTION: Values stored in Map ===
const map = new Map<string, any>();
map.set("undef", undefined);
map.set("nul", null);
map.set("t", true);
map.set("f", false);
map.set("zero", 0);
map.set("nan", NaN);
map.set("inf", Infinity);
map.set("num", 42);
map.set("str", "hello");
map.set("arr", [1, 2, 3]);

console.log("map.undef:", map.get("undef"));
console.log("map.nul:", map.get("nul"));
console.log("map.t:", map.get("t"));
console.log("map.f:", map.get("f"));
console.log("map.zero:", map.get("zero"));
console.log("map.nan:", Number.isNaN(map.get("nan")));
console.log("map.inf:", map.get("inf"));
console.log("map.num:", map.get("num"));
console.log("map.str:", map.get("str"));

// === SECTION: Values passed through function calls ===
function identity(x: any): any {
  return x;
}

console.log("fn(undefined):", identity(undefined));
console.log("fn(null):", identity(null));
console.log("fn(true):", identity(true));
console.log("fn(false):", identity(false));
console.log("fn(0):", identity(0));
console.log("fn(NaN):", Number.isNaN(identity(NaN)));
console.log("fn(42):", identity(42));
console.log("fn(3.14):", identity(3.14));
console.log("fn(str):", identity("hello"));
console.log("fn(Infinity):", identity(Infinity));

// === SECTION: Truthiness for every type ===
console.log("truthy undefined:", !!undefined);
console.log("truthy null:", !!null);
console.log("truthy true:", !!true);
console.log("truthy false:", !!false);
console.log("truthy 0:", !!0);
console.log("truthy NaN:", !!NaN);
console.log("truthy empty str:", !!"");
console.log("truthy 1:", !!1);
console.log("truthy -1:", !!(-1));
console.log("truthy str:", !!"hello");
console.log("truthy []:", !![]);
console.log("truthy {}:", !!{});
console.log("truthy Infinity:", !!Infinity);

// === SECTION: Equality semantics ===
console.log("null == undefined:", null == undefined);
console.log("null === undefined:", null === undefined);
console.log("0 == false:", 0 == false);
console.log("'' == false:", "" == false);
console.log("NaN == NaN:", NaN == NaN);
console.log("NaN === NaN:", NaN === NaN);
console.log("0 === -0:", 0 === -0);
console.log("Infinity === Infinity:", Infinity === Infinity);

// === SECTION: String conversion for every type ===
console.log("String(undefined):", String(undefined));
console.log("String(null):", String(null));
console.log("String(true):", String(true));
console.log("String(false):", String(false));
console.log("String(0):", String(0));
console.log("String(-0):", String(-0));
console.log("String(NaN):", String(NaN));
console.log("String(42):", String(42));
console.log("String(3.14):", String(3.14));
console.log("String(Infinity):", String(Infinity));
console.log("String(-Infinity):", String(-Infinity));

// === SECTION: Number conversion for every type ===
console.log("Number(undefined):", Number(undefined));
console.log("Number(null):", Number(null));
console.log("Number(true):", Number(true));
console.log("Number(false):", Number(false));
console.log("Number(''):", Number(""));
console.log("Number('42'):", Number("42"));
console.log("Number('hello'):", Number("hello"));

// === SECTION: Values survive JSON roundtrip ===
const jsonTest = { a: 1, b: "two", c: true, d: null, e: [1, 2, 3], f: { g: 4 } };
const roundtrip = JSON.parse(JSON.stringify(jsonTest));
console.log("json.a:", roundtrip.a);
console.log("json.b:", roundtrip.b);
console.log("json.c:", roundtrip.c);
console.log("json.d:", roundtrip.d);
console.log("json.e:", roundtrip.e);
console.log("json.f.g:", roundtrip.f.g);

// === SECTION: Edge number values ===
console.log("MAX_SAFE_INTEGER:", Number.MAX_SAFE_INTEGER);
console.log("MIN_SAFE_INTEGER:", Number.MIN_SAFE_INTEGER);
console.log("MAX_VALUE:", Number.MAX_VALUE);
console.log("EPSILON:", Number.EPSILON);
console.log("isFinite(42):", Number.isFinite(42));
console.log("isFinite(Infinity):", Number.isFinite(Infinity));
console.log("isFinite(NaN):", Number.isFinite(NaN));
console.log("isNaN(NaN):", Number.isNaN(NaN));
console.log("isNaN(42):", Number.isNaN(42));
console.log("isInteger(42):", Number.isInteger(42));
console.log("isInteger(42.5):", Number.isInteger(42.5));
console.log("isSafeInteger(MAX):", Number.isSafeInteger(Number.MAX_SAFE_INTEGER));
console.log("isSafeInteger(MAX+1):", Number.isSafeInteger(Number.MAX_SAFE_INTEGER + 1));
