// Test advanced JSON: pretty-print, replacer, reviver, edge cases, toJSON

// Pretty-print with spaces
const obj = { a: 1, b: "hello", c: [1, 2, 3] };
const pretty = JSON.stringify(obj, null, 2);
console.log(pretty);
// {
//   "a": 1,
//   "b": "hello",
//   "c": [
//     1,
//     2,
//     3
//   ]
// }

// Pretty-print with tab
const tabbed = JSON.stringify({ x: 1 }, null, "\t");
console.log(tabbed);
// {
// 	"x": 1
// }

// Replacer function
const withReplacer = JSON.stringify(
  { name: "Alice", password: "secret", age: 30 },
  (key: string, value: any) => {
    if (key === "password") return undefined;
    return value;
  }
);
console.log(withReplacer); // {"name":"Alice","age":30}

// Key whitelist (replacer as array)
const whitelist = JSON.stringify(
  { name: "Bob", age: 25, city: "NYC" },
  ["name", "city"]
);
console.log(whitelist); // {"name":"Bob","city":"NYC"}

// Reviver function
const revived = JSON.parse(
  '{"date":"2024-01-15","count":"42"}',
  (key: string, value: any) => {
    if (key === "count") return Number(value);
    return value;
  }
);
console.log(revived.count); // 42
console.log(typeof revived.count); // number
console.log(revived.date); // 2024-01-15

// JSON.stringify(undefined) returns undefined (not a string)
const undefinedResult = JSON.stringify(undefined);
console.log(undefinedResult); // undefined
console.log(undefinedResult === undefined); // true

// undefined values in objects are dropped
const withUndefined = JSON.stringify({ a: 1, b: undefined, c: 3 });
console.log(withUndefined); // {"a":1,"c":3}

// undefined values in arrays become null
const arrWithUndefined = JSON.stringify([1, undefined, 3]);
console.log(arrWithUndefined); // [1,null,3]

// Circular reference should throw
let circular: any = { a: 1 };
circular.self = circular;
let threw = false;
try {
  JSON.stringify(circular);
} catch (e: any) {
  threw = true;
  console.log(e instanceof TypeError); // true
}
console.log(threw); // true

// toJSON method on objects
const withToJSON = {
  raw: 12345,
  toJSON() {
    return { formatted: "12,345" };
  }
};
const toJSONResult = JSON.stringify(withToJSON);
console.log(toJSONResult); // {"formatted":"12,345"}

// Nested toJSON
const nested = {
  inner: {
    value: 42,
    toJSON() {
      return "custom-42";
    }
  }
};
console.log(JSON.stringify(nested)); // {"inner":"custom-42"}

// Expected output:
// {
//   "a": 1,
//   "b": "hello",
//   "c": [
//     1,
//     2,
//     3
//   ]
// }
// {
// 	"x": 1
// }
// {"name":"Alice","age":30}
// {"name":"Bob","city":"NYC"}
// 42
// number
// 2024-01-15
// undefined
// true
// {"a":1,"c":3}
// [1,null,3]
// true
// true
// {"formatted":"12,345"}
// {"inner":"custom-42"}
