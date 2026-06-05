function errorName(fn: () => void): string {
  try {
    fn();
    return "none";
  } catch (err) {
    return (err as Error).name;
  }
}

const order: string[] = [];
const nested = JSON.parse('{"a":1,"b":{"c":2},"d":3}', function (key, value) {
  order.push(key + ":" + typeof value);
  if (key === "c") return 4;
  return value;
});
console.log("post order:", order.join("|"), nested.b.c);

const deleted = JSON.parse('{"a":1,"b":2}', function (key, value) {
  if (key === "a") return undefined;
  return value;
});
console.log(
  "object delete:",
  Object.keys(deleted).join(","),
  "a" in deleted,
  deleted.a === undefined,
);

let seenB = "";
const proto = { b: 99 };
const reread = JSON.parse('{"a":1,"b":2}', function (key, value) {
  if (key === "a") {
    Object.setPrototypeOf(this, proto);
    delete this.b;
    return value;
  }
  if (key === "b") {
    seenB = String(value);
  }
  return value;
});
console.log("holder reread:", seenB, reread.b, Object.keys(reread).join(","));

const array = JSON.parse("[1,2,3]", function (key, value) {
  if (key === "1") return undefined;
  return value;
});
console.log(
  "array delete:",
  array.length,
  Object.keys(array).join(","),
  1 in array,
  array[1] === undefined,
);

let rootThisIsHolder = false;
const root = JSON.parse('{"x":1}', function (key, value) {
  if (key === "") {
    rootThisIsHolder = this[""] === value;
  }
  return value;
});
console.log("root this:", rootThisIsHolder, root.x);

const rootDeleted = JSON.parse('{"x":1}', function (key, value) {
  if (key === "") return undefined;
  return value;
});
console.log("root delete:", rootDeleted === undefined);

console.log(
  "getter abrupt:",
  errorName(() => {
    JSON.parse('{"a":1,"b":2}', function (key, value) {
      if (key === "a") {
        Object.defineProperty(this, "b", {
          get() {
            throw new TypeError("boom");
          },
          enumerable: true,
          configurable: true,
        });
      }
      return value;
    });
  }),
);

const deleteFalse = JSON.parse('{"a":1}', function (key, value) {
  if (key === "a") {
    Object.defineProperty(this, "a", { value, configurable: false });
    return undefined;
  }
  return value;
});
console.log(
  "delete false:",
  "a" in deleteFalse,
  deleteFalse.a,
);

const defineFalse = JSON.parse('{"a":1}', function (key, value) {
  if (key === "a") {
    Object.defineProperty(this, "a", {
      value: 0,
      writable: false,
      configurable: false,
    });
    return 9;
  }
  return value;
});
console.log(
  "define false:",
  "a" in defineFalse,
  defineFalse.a,
);

console.log(
  "reviver throw:",
  errorName(() => {
    JSON.parse('{"a":1}', function (key, value) {
      if (key === "a") throw new RangeError("stop");
      return value;
    });
  }),
);
