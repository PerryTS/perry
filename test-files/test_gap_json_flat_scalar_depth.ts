// Large flat scalar arrays can skip the nesting state machine, but must still
// receive normal syntax validation below and above the lazy-parser threshold.
let checked = 0;
let failures = 0;
const valid = ["1,2,3]", "true,false,null]", "-0,1.25e2]"];
const expected = ["[1,2,3]", "[true,false,null]", "[0,125]"];
const invalid = ["1,]", "01]", "1.]", "+1]", "1e+]", "NaN]", "Infinity]", "1 2]", "1,2", "1]true", "1\x00]"];
for (const length of [256, 1024, 4096]) {
  const prefix = "[" + " ".repeat(length);
  for (let i = 0; i < valid.length; i++) {
    try {
      const result: any = JSON.parse(prefix + valid[i]);
      // Inspect parsed values through a fresh array: the baseline lazy-array
      // stringify shortcut retains source whitespace, independently of parsing.
      const materialized: any[] = [];
      for (let j = 0; j < result.length; j++) materialized.push(result[j]);
      if (JSON.stringify(materialized) !== expected[i]) failures++;
    } catch {
      failures++;
    }
    checked++;
  }
  for (const tail of invalid) {
    let rejected = false;
    try {
      JSON.parse(prefix + tail);
    } catch (error: any) {
      rejected = error.name === "SyntaxError";
    }
    if (!rejected) failures++;
    checked++;
  }
}
console.log("flat scalar validation", checked, failures);
