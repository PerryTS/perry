// A lone-surrogate string key keeps its value and position.
const o: any = { "\uD800": 1, b: 2, "\uDC00": 3 };
console.log(Object.keys(o).map((k) => k.length + ":" + k.charCodeAt(0).toString(16)).join(","));
console.log(o["\uD800"], o.b, o["\uDC00"], o[""]);
const m: any = { "\uD800"() { return 7; }, "\uDC01": 8 };
console.log(m["\uD800"](), m["\uDC01"], Object.keys(m).length, m[""]);
const s = 5;
const p: any = { ...{ x: 1 }, "\uD800": s };
console.log(JSON.stringify(Object.keys(p)), p["\uD800"], p[""]);
