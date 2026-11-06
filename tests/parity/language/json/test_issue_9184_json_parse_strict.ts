// #9184: JSON.parse must validate while DirectParser builds the value. These
// cases intentionally cover truncation, separators, numbers, strings, and
// trailing roots so removing an external validation pass stays safe.
const bs = String.fromCharCode(92);
const invalid: string[] = [
  "{",
  "}",
  "[",
  "]",
  "",
  "   ",
  "{,}",
  "[,]",
  "[1,]",
  "{\"a\":1,}",
  "{a:1}",
  "{'a':1}",
  "[01]",
  "[-01]",
  "[1.]",
  "[.5]",
  "[+1]",
  "[1e]",
  "[1e+]",
  "[--1]",
  "[NaN]",
  "[Infinity]",
  "[-Infinity]",
  "[undefined]",
  "[TRUE]",
  "\"unterminated",
  "[\"bad" + bs + "x\"]",
  "[\"" + bs + "u12\"]",
  "[\"" + bs + "uZZZZ\"]",
  "{\"a\" 1}",
  "{\"a\":}",
  "{:1}",
  "[1 2]",
  "[1][2]",
  "{}{}",
  "nul",
  "tru",
  "[1,,2]",
  "{\"a\":1 \"b\":2}",
  "\"" + bs + "t\"x",
  "\"\t\"",
  "\"abcdefghijklmnop\nqrst\"",
  "\vnull",
];

const valid: [string, string][] = [
  ["{}", "{}"],
  ["[]", "[]"],
  ["0", "0"],
  ["-0", "0"],
  ["1e5", "100000"],
  ["1E+5", "100000"],
  ["1e-5", "0.00001"],
  ["-1.5", "-1.5"],
  ["null", "null"],
  ["true", "true"],
  ["false", "false"],
  ["\"\"", "\"\""],
  ["\"" + bs + "u0041\"", "\"A\""],
  ["\"" + bs + "n\"", "\"" + bs + "n\""],
  ["[1,2,3]", "[1,2,3]"],
  ["{\"a\":{\"b\":[1,{\"c\":null}]}}", "{\"a\":{\"b\":[1,{\"c\":null}]}}"],
  ["{\"a\":1,\"a\":2}", "{\"a\":2}"],
  ["[[[[[1]]]]]", "[[[[[1]]]]]"],
  ["\"" + bs + "ud83d" + bs + "ude00\"", "\"😀\""],
  ["\"" + bs + "ud800\"", "\"" + bs + "ud800\""],
  ["\"" + bs + "ud800" + bs + "u0041\"", "\"" + bs + "ud800A\""],
  ["\"" + bs + "udc00\"", "\"" + bs + "udc00\""],
  ["{\"\":1}", "{\"\":1}"],
  ["  {\"a\" : 1 }  ", "{\"a\":1}"],
  ["[1e308]", "[1e+308]"],
  ["[-1e308]", "[-1e+308]"],
  ["[1e-400]", "[0]"],
  ["9007199254740993", "9007199254740992"],
];

let correctThrows = 0;
let invalidFailures = 0;
for (let i = 0; i < invalid.length; i++) {
  try {
    JSON.parse(invalid[i]);
    invalidFailures++;
  } catch (e: any) {
    if (e && e.constructor && e.constructor.name === "SyntaxError") {
      correctThrows++;
    } else {
      invalidFailures++;
    }
  }
}

let correctValues = 0;
let validFailures = 0;
for (let i = 0; i < valid.length; i++) {
  try {
    const actual = JSON.stringify(JSON.parse(valid[i][0]));
    if (actual === valid[i][1]) correctValues++;
    else {
      console.log("valid-mismatch:" + i + ":" + actual + ":" + valid[i][1]);
      validFailures++;
    }
  } catch (_e: any) {
    validFailures++;
  }
}

console.log("invalid:" + correctThrows + "/" + invalid.length + ":" + invalidFailures);
console.log("valid:" + correctValues + "/" + valid.length + ":" + validFailures);

interface StrictRow {
  id: number;
}

let typedError = "none";
try {
  JSON.parse<StrictRow[]>("[{\"id\":1},]");
} catch (e: any) {
  if (e && e.constructor) typedError = e.constructor.name;
}
const typedRows = JSON.parse<StrictRow[]>("[{\"id\":1},{\"id\":2}]");
console.log(
  "typed:" +
    typedError +
    ":" +
    typedRows.length +
    ":" +
    typedRows[0].id +
    ":" +
    typedRows[1].id,
);
