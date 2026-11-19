// A class declaration in a function called more than once, with an instance
// member keyed by a well-known or registered symbol and no other
// per-evaluation state (#11043: mongodb's `List`, whose
// `get [Symbol.toStringTag]` and `*[Symbol.iterator]` sit before `push`).
// Every evaluation's prototype holds every member, the string-keyed ones after
// the symbol member included.

function makeList() {
  class List {
    count = 0;
    get length() {
      return this.count;
    }
    get [Symbol.toStringTag]() {
      return "List";
    }
    *[Symbol.iterator]() {
      for (let i = 0; i < this.count; i++) yield i;
    }
    [Symbol.for("list.tag")]() {
      return "tagged";
    }
    set [Symbol.for("list.count")](n: number) {
      this.count = n;
    }
    push() {
      this.count += 1;
    }
  }
  return List;
}

const evaluations = [makeList(), makeList(), makeList()];
for (const List of evaluations) {
  const list: any = new List();
  list.push();
  list.push();
  console.log(Object.getOwnPropertyNames(List.prototype).join(","));
  console.log(Object.prototype.toString.call(list), list.length, [...list].join(","));
  console.log(list[Symbol.for("list.tag")](), typeof list.push);
}

// The first evaluation's prototype also holds the symbol-keyed members as
// own properties, the accessor's setter included.
const First: any = evaluations[0];
const first: any = new First();
console.log(Object.getOwnPropertySymbols(First.prototype).map(String).join(","));
first[Symbol.for("list.count")] = 5;
console.log(first.length);
