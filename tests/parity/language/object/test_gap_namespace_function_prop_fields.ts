// A program field stored on a built-in namespace, a built-in constructor or
// a native function value under a key the runtime does not declare is read
// back by the program, through the same object or a value that may be it
// (a computed read of a dictionary can yield the inherited `constructor`).
class Item {
  n: number;
  constructor(n: number) {
    this.n = n;
  }
}

const tag = new Item(1);
(Math as any).progTag = tag;
(Object as any).progTag = new Item(2);
const push: any = [].push;
push.progTag = new Item(3);

console.log((Math as any).progTag.n, (Object as any).progTag.n);
console.log(([] as any).push.progTag.n, ([] as any).pop.progTag);
console.log(typeof (Array as any).progTag, typeof (JSON as any).progTag);

const dict: { [k: string]: any } = {};
const key = process.argv.length > 99 ? "x" : "constructor";
const v = dict[key];
console.log(v === Object, v.progTag.n);
v.progTag.n = 20;
console.log((Object as any).progTag.n);

// Declared keys keep their meaning.
const limit = (Error as any).stackTraceLimit;
(Error as any).stackTraceLimit = 5;
console.log(typeof limit, (Error as any).stackTraceLimit);
(Error as any).stackTraceLimit = limit;

// A field on what a lookup returned: the program object or the namespace.
class SF {
  lineMap: number[] | undefined;
  constructor() {
    this.lineMap = undefined;
  }
}
function lines(sf: any): number[] {
  return sf.lineMap || (sf.lineMap = [0, 1, 2]);
}
console.log(lines(new SF()).length, lines(v).length, (Object as any).lineMap.length);
const keys = Object.keys(Object).filter((k) => k === "progTag" || k === "lineMap");
console.log(keys.join(","));
