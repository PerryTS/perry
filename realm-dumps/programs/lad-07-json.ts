const src = '{"name":"perry","n":3,"f":1.5,"neg":-2e3,"arr":[1,"two",null,true,{"x":[]}],"esc":"a\\"b\\n\\u00e9"}';
const o = JSON.parse(src);
console.log(o.name, o.n, o.f, o.neg, o.arr.length, o.arr[4].x, o.esc);
console.log(JSON.stringify(o));
console.log(JSON.stringify({ a: [1, { b: 2 }], c: "x" }, null, 2));
console.log(JSON.stringify({ u: undefined, f() {}, d: new Date(0), n: NaN }));
console.log(JSON.parse('[1,2,3]', (k, v) => typeof v === "number" ? v * 10 : v));
console.log(JSON.stringify({ b: 1, a: 2 }, ["a"]), JSON.stringify("é😀"), JSON.stringify({ toJSON() { return 5; } }));
