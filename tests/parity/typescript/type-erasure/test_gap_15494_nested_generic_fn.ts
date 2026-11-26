// #15494: a nested generic function declaration and a generic function
// expression lower their own type parameters. A shadowed `T` names a
// different type than the enclosing one, and a `T` binds to whatever the
// caller passes, so neither may narrow the runtime value.

function outer<T>(saved: T) {
  function first<T>(items: T[]): T {
    console.log("saved", saved, typeof saved);
    return items[0];
  }
  const got = first(["x"]);
  console.log("first", got, typeof got);
  console.log("first number", first([1, 2]) + 1);
  return first([saved]);
}
console.log("outer", outer(42));
console.log("outer string", outer("s"));

function pairs() {
  function swap<A, B>(pair: [A, B]): [B, A] {
    return [pair[1], pair[0]];
  }
  function pick<O, K extends keyof O>(o: O, k: K): O[K] {
    return o[k];
  }
  function wrap<T>(value: T): { value: T } {
    return { value };
  }
  console.log("swap", swap([1, "a"]), swap(["b", true]));
  console.log("pick", pick({ a: 1, b: "two" }, "b"));
  console.log("wrap", wrap(3).value + 1, wrap("3").value + 1);
}
pairs();

function expressions() {
  const id = function <T>(x: T): T {
    return x;
  };
  const named = function self<T>(x: T, depth: number): T {
    return depth > 0 ? self(x, depth - 1) : x;
  };
  console.log("id", id(5) + 1, id("5") + 1, id([1]).length);
  console.log("named", named("deep", 3), named(7, 2) * 2);
}
expressions();

function constrained() {
  function longest<T extends { length: number }>(a: T, b: T): T {
    return a.length >= b.length ? a : b;
  }
  console.log("longest", longest("ab", "abc"), longest([1, 2], [3]));
  const fake: any = { length: 9, tag: "obj" };
  console.log("longest object", longest(fake, "short").tag);
}
constrained();

// An explicit type argument specializes `specialized`; its inner `first`
// redeclares `T`, so it keeps taking and returning the caller's strings.
function specialized<T>(saved: T) {
  function first<T>(items: T[]): T {
    return items[0];
  }
  const keep = (x: T): T => x;
  const got = first(["x", "y"]);
  console.log("specialized", got + 1, typeof got, got.length, keep(saved));
}
specialized<number>(42);
specialized<string>("s");
