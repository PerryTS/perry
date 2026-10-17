// Object spread (CopyDataProperties) copies own enumerable properties only.
// A private entry in an object's shape (a class's #field, a built-in's
// internal slot) is not a property and must never be copied.

class Point {
  #secret = 41;
  x = 1;
  y = 2;
  get secret() { return this.#secret + 1; }
}
const pt: any = new Point();
const ptCopy = { ...pt };
console.log("class", JSON.stringify(Object.keys(ptCopy)), JSON.stringify(ptCopy), pt.secret);

const withPrefix = { a: 0, ...pt, z: 3 };
console.log("class prefixed", JSON.stringify(Object.keys(withPrefix)));

const re: any = /ab+c/gi;
re.extra = "e";
console.log("regexp", JSON.stringify(Object.keys({ ...re })), JSON.stringify({ ...re }));

const url: any = new URL("https://example.com:8080/p?q=1#h");
console.log("url", JSON.stringify(Object.keys({ ...url })));

const sp: any = new URLSearchParams("a=1&b=2");
console.log("searchParams", JSON.stringify(Object.keys({ ...sp })), JSON.stringify({ ...sp }));
console.log("searchParams from url", JSON.stringify(Object.keys({ ...url.searchParams })));

// Object.assign agrees with spread.
console.log("assign", JSON.stringify(Object.keys(Object.assign({}, pt))), JSON.stringify(Object.keys(Object.assign({}, sp))));

// Rest destructuring also copies own enumerable properties only.
const { x, ...restPt } = pt;
console.log("rest", x, JSON.stringify(Object.keys(restPt)));
const hidden: any = { a: 1, b: 2 };
Object.defineProperty(hidden, "c", { value: 3, enumerable: false });
const { a, ...restHidden } = hidden;
console.log("rest non-enumerable", a, JSON.stringify(Object.keys(restHidden)), JSON.stringify(Object.keys({ ...hidden })));
const { href, ...restUrl } = url;
console.log("rest url", JSON.stringify(Object.keys(restUrl)));
const { size, ...restSp } = sp;
console.log("rest searchParams", size, JSON.stringify(Object.keys(restSp)));
