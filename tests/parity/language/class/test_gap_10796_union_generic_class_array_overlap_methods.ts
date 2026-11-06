// #10796: the same guard covers the whole "shares a name with
// Array.prototype" method set — find, findIndex, findLast, findLastIndex,
// map, filter, some, every, forEach, reduce, reduceRight, join, plus the
// mutators push/pop/shift/unshift — not just `find`. This fixture exercises
// a representative few of them (find, map, filter, forEach, reduce, push)
// on ONE receiver so a fix verified only on `find` can't pass here while
// leaving the others silently folding to the array fast path.
//
// It also pins the real-world trigger: the receiver's static type is a
// `Union` containing a *generic* class instance (`Box<string> | undefined`),
// matching cheerio's `searchContext: Cheerio<AnyNode> | undefined` in
// `load.ts` — a `Type::Named`-only fix would pass a simpler
// `Foo | undefined` test while leaving `Cheerio<AnyNode> | undefined`
// (and so cheerio itself) still misrouting through `Type::Generic` inside
// the union.
//
// `Box<T>` is shaped like cheerio's `Cheerio<T>` on purpose (`length` +
// a numeric index signature — "array-like" is exactly the shape that makes
// the array fast path plausible in the first place).
class Box<T> {
  length = 0;
  [index: number]: T;
  label: string;

  constructor(label: string) {
    this.label = label;
  }

  find(selector: string): string {
    return `${this.label}.find(${selector})`;
  }
  map(selector: string): string {
    return `${this.label}.map(${selector})`;
  }
  filter(selector: string): string {
    return `${this.label}.filter(${selector})`;
  }
  forEach(selector: string): string {
    return `${this.label}.forEach(${selector})`;
  }
  reduce(selector: string): string {
    return `${this.label}.reduce(${selector})`;
  }
  push(selector: string): string {
    return `${this.label}.push(${selector})`;
  }
}

function make(flag: boolean): Box<string> | undefined {
  return flag ? new Box<string>("box") : undefined;
}

const b: Box<string> | undefined = make(true);
if (!b) {
  throw new Error("unreachable");
}

const results: string[] = [];
results.push(b.find("a"));
results.push(b.map("b"));
results.push(b.filter("c"));
results.push(b.forEach("d"));
results.push(b.reduce("e"));
results.push(b.push("f"));
console.log(results.join(" | "));
