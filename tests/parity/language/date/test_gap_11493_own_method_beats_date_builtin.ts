// #11493: an own method beats a Date (or Array) builtin whatever the compiler
// proved about the receiver.
//
// #10943 made an own property beat the builtin on a receiver whose KIND is
// proven. The other half is #10476's receiver-kind guard, which serves an
// UNPROVEN receiver: it checks at runtime that the value is a Date (or a plain
// array) and then called the builtin directly. A matching kind says nothing
// about own properties, so `const d: any = new Date(0); d.getTime = () => 42;
// d.getTime()` printed 0. The same held for every Date method that guard
// lowers, for its `toLocaleString()` arm, and for its array methods.
//
// Also covered: the proven-Date folds that were missing from the #10943 table
// (`getTimezoneOffset`, `toJSON`, `toDateString`, `toTimeString`, the three
// `toLocale*String`s), proven-Date names the codegen chain lowers directly
// (`getUTCHours`, `setTime`, reached here through a class field), and the
// proven-array names the kind guard handles.
//
// Not covered: `toUTCString`/`toGMTString` on a Date that HIR proves. HIR
// folds both spellings into one node, so the guard cannot tell which name to
// test. (The unproven rows below do cover both.)
//
// Every native row avoids local-time and locale output, so this file is
// byte-identical to node in any TZ.

function t(label: string, f: () => unknown) {
  try {
    console.log(label + "=" + String(f()));
  } catch (e: any) {
    console.log(label + "=throw:" + e.constructor.name);
  }
}

// --- the issue: an unproven Date receiver ------------------------------------
const d1: any = new Date(0);
d1.getTime = () => 42;
t("any-annotated local", () => d1.getTime());

function viaParam(d) { return d.getTime(); }
const d2 = new Date(0);
(d2 as any).getTime = () => 43;
t("untyped parameter", () => viaParam(d2));

const holder: any = { d: new Date(0) };
holder.d.getTime = () => 44;
t("property read", () => holder.d.getTime());

const list: any[] = [new Date(0)];
list[0].getTime = () => 45;
t("array element", () => list[0].getTime());

let made = 0;
function make(): any { made++; const d = new Date(0); (d as any).getTime = () => 46; return d; }
t("call result, evaluated once", () => make().getTime() + "/" + made);

// --- every Date method the kind guard lowers, on an unproven receiver --------
const u: any = new Date(0);
u.getTimezoneOffset = () => "own-getTimezoneOffset";
u.getFullYear = () => "own-getFullYear";
u.getMonth = () => "own-getMonth";
u.getDate = () => "own-getDate";
u.getDay = () => "own-getDay";
u.getHours = () => "own-getHours";
u.getMinutes = () => "own-getMinutes";
u.getSeconds = () => "own-getSeconds";
u.getMilliseconds = () => "own-getMilliseconds";
u.getUTCFullYear = () => "own-getUTCFullYear";
u.getUTCMonth = () => "own-getUTCMonth";
u.getUTCDate = () => "own-getUTCDate";
u.getUTCDay = () => "own-getUTCDay";
u.getUTCHours = () => "own-getUTCHours";
u.getUTCMinutes = () => "own-getUTCMinutes";
u.getUTCSeconds = () => "own-getUTCSeconds";
u.getUTCMilliseconds = () => "own-getUTCMilliseconds";
u.toISOString = () => "own-toISOString";
u.toDateString = () => "own-toDateString";
u.toTimeString = () => "own-toTimeString";
u.toUTCString = () => "own-toUTCString";
u.toGMTString = () => "own-toGMTString";
u.toLocaleDateString = () => "own-toLocaleDateString";
u.toLocaleTimeString = () => "own-toLocaleTimeString";
u.setFullYear = (x) => "own-setFullYear:" + x;
u.setMonth = (x) => "own-setMonth:" + x;
u.setDate = (x) => "own-setDate:" + x;
u.setHours = (x) => "own-setHours:" + x;
u.setMinutes = (x) => "own-setMinutes:" + x;
u.setSeconds = (x) => "own-setSeconds:" + x;
u.setMilliseconds = (x) => "own-setMilliseconds:" + x;
u.setTime = (x) => "own-setTime:" + x;
u.setUTCFullYear = (x) => "own-setUTCFullYear:" + x;
u.setUTCMonth = (x) => "own-setUTCMonth:" + x;
u.setUTCDate = (x) => "own-setUTCDate:" + x;
u.setUTCHours = (x) => "own-setUTCHours:" + x;
u.setUTCMinutes = (x) => "own-setUTCMinutes:" + x;
u.setUTCSeconds = (x) => "own-setUTCSeconds:" + x;
u.setUTCMilliseconds = (x) => "own-setUTCMilliseconds:" + x;
t("unproven getTimezoneOffset", () => u.getTimezoneOffset());
t("unproven getFullYear", () => u.getFullYear());
t("unproven getMonth", () => u.getMonth());
t("unproven getDate", () => u.getDate());
t("unproven getDay", () => u.getDay());
t("unproven getHours", () => u.getHours());
t("unproven getMinutes", () => u.getMinutes());
t("unproven getSeconds", () => u.getSeconds());
t("unproven getMilliseconds", () => u.getMilliseconds());
t("unproven getUTCFullYear", () => u.getUTCFullYear());
t("unproven getUTCMonth", () => u.getUTCMonth());
t("unproven getUTCDate", () => u.getUTCDate());
t("unproven getUTCDay", () => u.getUTCDay());
t("unproven getUTCHours", () => u.getUTCHours());
t("unproven getUTCMinutes", () => u.getUTCMinutes());
t("unproven getUTCSeconds", () => u.getUTCSeconds());
t("unproven getUTCMilliseconds", () => u.getUTCMilliseconds());
t("unproven toISOString", () => u.toISOString());
t("unproven toDateString", () => u.toDateString());
t("unproven toTimeString", () => u.toTimeString());
t("unproven toUTCString", () => u.toUTCString());
t("unproven toGMTString", () => u.toGMTString());
t("unproven toLocaleDateString", () => u.toLocaleDateString());
t("unproven toLocaleTimeString", () => u.toLocaleTimeString());
t("unproven setFullYear", () => u.setFullYear(1));
t("unproven setMonth", () => u.setMonth(1));
t("unproven setDate", () => u.setDate(1));
t("unproven setHours", () => u.setHours(1, 2));
t("unproven setMinutes", () => u.setMinutes(1));
t("unproven setSeconds", () => u.setSeconds(1));
t("unproven setMilliseconds", () => u.setMilliseconds(1));
t("unproven setTime", () => u.setTime(1));
t("unproven setUTCFullYear", () => u.setUTCFullYear(1));
t("unproven setUTCMonth", () => u.setUTCMonth(1));
t("unproven setUTCDate", () => u.setUTCDate(1));
t("unproven setUTCHours", () => u.setUTCHours(1));
t("unproven setUTCMinutes", () => u.setUTCMinutes(1));
t("unproven setUTCSeconds", () => u.setUTCSeconds(1));
t("unproven setUTCMilliseconds", () => u.setUTCMilliseconds(1));

// --- `toLocaleString()`: the kind guard's own arm ----------------------------
function loc(x) { return x.toLocaleString(); }
const dl = new Date(0);
(dl as any).toLocaleString = () => "own-toLocaleString";
t("toLocaleString own on a Date", () => loc(dl));
t("toLocaleString on a string", () => loc("abc"));
t("toLocaleString on a Symbol", () => loc(Symbol("s")));

// --- what the own method sees, and how it is read -----------------------------
const th: any = new Date(0);
th.setHours = function (h, m) { return (this === th) + ":" + h + ":" + m; };
t("own method this and arguments", () => th.setHours(1, 2));

const ac: any = new Date(0);
let gets = 0;
Object.defineProperty(ac, "getTime", { get() { gets++; return () => "accessor"; } });
t("own accessor runs once", () => ac.getTime() + "/" + gets);

const nc: any = new Date(0);
nc.getTime = 5;
t("own non-callable throws", () => nc.getTime());

const del: any = new Date(7);
del.getTime = () => "own";
t("before delete", () => del.getTime());
delete del.getTime;
t("after delete", () => del.getTime());

const bor: any = new Date(9);
bor.getTime = Date.prototype.getTime;
t("borrowed builtin", () => bor.getTime());

// installed partway through a hot call site
function hot(d) { return d.getTime(); }
const hd: any = new Date(5);
let seen = "";
for (let i = 0; i < 4; i++) {
  if (i === 2) hd.getTime = () => 100;
  seen += hot(hd) + ",";
}
t("installed mid-loop", () => seen);

// --- nothing shadowed: the builtin still runs --------------------------------
const plain: any = new Date(86400000 + 3600000 * 5 + 60000 * 6 + 7008);
t("native getTime", () => plain.getTime());
t("native getUTCFullYear", () => plain.getUTCFullYear());
t("native getUTCHours", () => plain.getUTCHours());
t("native toISOString", () => plain.toISOString());
t("native setUTCMinutes", () => plain.setUTCMinutes(30));
t("native setTime", () => plain.setTime(0) + "/" + plain.getTime());
t("native call result", () => new Date(3).getTime());

// --- proven Dates: the folds that were missing from the #10943 table ---------
const p = new Date(0);
(p as any).getTimezoneOffset = () => "own-getTimezoneOffset";
(p as any).toJSON = () => "own-toJSON";
(p as any).toDateString = () => "own-toDateString";
(p as any).toTimeString = () => "own-toTimeString";
(p as any).toLocaleDateString = () => "own-toLocaleDateString";
(p as any).toLocaleTimeString = () => "own-toLocaleTimeString";
(p as any).toLocaleString = () => "own-toLocaleString";
t("proven getTimezoneOffset", () => p.getTimezoneOffset());
t("proven toJSON", () => p.toJSON());
t("proven toDateString", () => p.toDateString());
t("proven toTimeString", () => p.toTimeString());
t("proven toLocaleDateString", () => p.toLocaleDateString());
t("proven toLocaleTimeString", () => p.toLocaleTimeString());
t("proven toLocaleString", () => p.toLocaleString());
const pn = new Date(0);
t("proven native toJSON", () => pn.toJSON());
t("proven native getTimezoneOffset is a number", () => typeof pn.getTimezoneOffset());
// the own value keeps its own type through the typed fold
const pt = new Date(0);
(pt as any).toDateString = () => 42;
t("own result keeps its type", () => typeof pt.toDateString() + "/" + (pt.toDateString() as any).length);

// a proven Date the codegen chain lowers directly
class Holder { d: Date = new Date(0); }
const h = new Holder();
(h.d as any).getUTCHours = () => "own-getUTCHours";
(h.d as any).setTime = (x) => "own-setTime:" + x;
t("class field getUTCHours", () => h.d.getUTCHours());
t("class field setTime", () => h.d.setTime(1));
const h2 = new Holder();
t("class field native", () => h2.d.getUTCHours() + "/" + h2.d.setTime(7));

// --- arrays: the same kind guard's plain-array arm ---------------------------
const ua: any = JSON.parse("[3,1,2]");
ua.toReversed = () => "own-toReversed";
ua.toSorted = () => "own-toSorted";
ua.toSpliced = () => "own-toSpliced";
ua.reduceRight = () => "own-reduceRight";
ua.copyWithin = () => "own-copyWithin";
ua.flat = () => "own-flat";
t("unproven array toReversed", () => ua.toReversed());
t("unproven array toSorted", () => ua.toSorted());
t("unproven array toSpliced", () => ua.toSpliced(0));
t("unproven array reduceRight", () => ua.reduceRight((x) => x));
t("unproven array copyWithin", () => ua.copyWithin(0));
t("unproven array flat", () => ua.flat());
const ub: any = JSON.parse("[3,[1],2]");
t("unproven array native toSorted", () => JSON.stringify(ub.toSorted()));
t("unproven array native flat", () => JSON.stringify(ub.flat()));
t("unproven array native reduceRight", () => ub.reduceRight((acc, x) => acc + "," + x));

const pa: any = [3, 1, 2];
pa.toReversed = () => "own-toReversed";
pa.toSorted = () => "own-toSorted";
pa.toSpliced = () => "own-toSpliced";
pa.reduceRight = () => "own-reduceRight";
pa.copyWithin = () => "own-copyWithin";
t("proven array toReversed", () => pa.toReversed());
t("proven array toSorted", () => pa.toSorted());
t("proven array toSpliced", () => pa.toSpliced(0));
t("proven array reduceRight", () => pa.reduceRight((x) => x));
t("proven array copyWithin", () => pa.copyWithin(0));
t("proven array native", () => JSON.stringify([3, 1, 2].toSorted()) + JSON.stringify([1, 2].toReversed()));
