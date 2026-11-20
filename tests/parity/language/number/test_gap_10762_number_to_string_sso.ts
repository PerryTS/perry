// #10762: `String(n)`, `${n}` and `n.toString()` return a short number's text
// as an inline short string (SSO) instead of a heap string. The text must not
// change, and every consumer must treat the SSO result exactly like the heap
// string it used to be: `.length`, equality against heap strings, property and
// Map/Set keys, string methods, JSON, joins, switch, `in`.

const values: number[] = [
  0, -0, 1, -1, 7, 42, 255, 256, -255, 9999, -9999, 10000, -10000, 99999,
  100000, 123456, 2147483647, -2147483648, 4294967296, 1e15, 1e21, 1e-6, 1e-7,
  0.5, -0.5, 1.25, 0.125, 0.1, 123.5, -12.5, 1 / 3, Number.MAX_VALUE,
  Number.MIN_VALUE, Number.EPSILON, Infinity, -Infinity, NaN,
];

function spellings(n: number): string[] {
  return [String(n), `${n}`, n.toString(), "" + n];
}

// 1. The text, and agreement between the four spellings.
for (const n of values) {
  const [a, b, c, d] = spellings(n);
  const agree = a === b && b === c && c === d;
  console.log(a, a.length, agree ? "agree" : `DISAGREE ${b} ${c} ${d}`);
}

// 2. Equality against heap-built strings of the same text.
const heapFortyTwo = ["4", "2"].join("");
const heapLong = "12" + "3456";
console.log(String(42) === heapFortyTwo, `${42}` === heapFortyTwo, (42).toString() === heapFortyTwo);
console.log(String(123456) === heapLong, heapFortyTwo === String(42));
console.log(String(42) < String(43), String(9) > String(10), String(5).localeCompare("5"));

// 3. As property keys, Map keys and Set members.
const obj: Record<string, number> = {};
const map = new Map<string, number>();
const set = new Set<string>();
for (let i = 0; i < 300; i++) {
  obj[String(i % 50)] = (obj[String(i % 50)] ?? 0) + 1;
  map.set(`${i % 60}`, (map.get(`${i % 60}`) ?? 0) + 1);
  set.add((i % 70).toString());
}
console.log(Object.keys(obj).length, obj["7"], obj[String(7)], "7" in obj, String(7) in obj);
console.log(map.size, map.get("59"), map.get(String(59)), map.has(`${59}`));
console.log(set.size, set.has("69"), set.has(String(69)), set.has("70"));

// 4. String methods on the result (receiver and argument positions).
const h = 7;
const m = 5;
console.log(String(h).padStart(2, "0") + ":" + `${m}`.padStart(2, "0"));
console.log((3).toString().padEnd(4, "*"), String(12345).split(""), `${-12}`.charCodeAt(0));
console.log(String(98765).at(-1), String(4321).slice(1, 3), String(42).concat(String(7), `${8}`));
console.log(String(123).indexOf("2"), `${1001}`.lastIndexOf("1"), String(55).startsWith("5"));
console.log(String(100).replace("0", "x"), String(12).repeat(3), String(-5).includes("-"));
console.log(String(0.5).toUpperCase(), (NaN).toString().toLowerCase(), String(77).codePointAt(1));

// 5. Serialisation, joins, concatenation chains, typeof.
const arr = [String(1), `${22}`, (333).toString(), String(4444), String(55555), String(666666)];
console.log(JSON.stringify(arr), arr.join("|"), arr.map((s) => s.length).join(","));
console.log(JSON.stringify({ [String(9)]: `${10}`, k: (11).toString() }));
console.log("id-" + String(5) + "-" + `${6}` + "-" + (7).toString(), typeof String(1), typeof `${2}`);
let acc = "";
for (let i = 0; i < 12; i++) acc += String(i);
console.log(acc, acc.length);

// 6. switch on the result, and truthiness.
function label(n: number): string {
  switch (String(n)) {
    case "0":
      return "zero";
    case "1":
      return "one";
    case "99999":
      return "max-sso";
    case "100000":
      return "min-heap";
    default:
      return "other";
  }
}
console.log(label(0), label(1), label(99999), label(100000), label(3));
console.log(!!String(0), !!`${0}`, String(0) ? "truthy" : "falsy");

// 7. Non-number arguments keep their old behaviour.
const sym = Symbol("s");
console.log(String(sym), String(null), String(undefined), String(true), `${false}`);
console.log(String("abc") === "abc", `${"abcdefgh"}`, String([1, 2, 3]), `${{ a: 1 }}`);
console.log(String({ toString() { return "custom"; } }), `${{ toString() { return "tpl"; } }}`);
console.log(String(10n), `${255n}`, (255n).toString(), [1, [2, 3]].toString());
try {
  console.log(`${sym}`);
} catch (e) {
  console.log("template symbol:", (e as Error).constructor.name);
}
for (const bad of [null, undefined]) {
  try {
    const v: any = bad;
    console.log(v.toString());
  } catch (e) {
    console.log("nullish toString:", (e as Error).constructor.name);
  }
}

// 8. Values that arrive dynamically typed.
const mixed: any[] = [5, "five", 5.5, -0, null, undefined, true, 12345, 123456, 1e21, [7], { x: 1 }];
console.log(mixed.map((v) => String(v)).join(","));
console.log(mixed.map((v) => `${v}`).join(","));
console.log(mixed.filter((v) => v !== null && v !== undefined).map((v) => v.toString()).join(","));
