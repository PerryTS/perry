// Issue #179 Step 2: edge-case coverage for lazy JSON parse.
// Must produce byte-for-byte identical output under PERRY_JSON_TAPE=1
// (lazy) vs the default direct parser, and match Node.

// 1. Empty array
const empty = JSON.parse('[]');
console.log("empty.length:" + empty.length);

// 2. Array of scalars
const scalars = JSON.parse('[1,2,3,4,5]') as number[];
console.log("scalars.length:" + scalars.length);
console.log("scalars[2]:" + scalars[2]);
let sum = 0;
for (let i = 0; i < scalars.length; i++) sum += scalars[i];
console.log("scalars.sum:" + sum);

// 3. Mixed types in an array
const mixed = JSON.parse('[1,"two",true,null,3.14]');
console.log("mixed.length:" + mixed.length);
console.log("mixed[0]:" + mixed[0]);
console.log("mixed[1]:" + mixed[1]);
console.log("mixed[2]:" + mixed[2]);
console.log("mixed[3]:" + mixed[3]);
console.log("mixed[4]:" + mixed[4]);

// 4. Deeply nested structure (4 levels)
const deep = JSON.parse('[{"a":{"b":{"c":{"d":42}}}}]');
console.log("deep[0].a.b.c.d:" + deep[0].a.b.c.d);

// 5. Unicode + escape sequences in strings
const uni = JSON.parse('[{"text":"\\u00e9\\u00e8\\u00ea","newline":"a\\nb","tab":"a\\tb","quote":"he said \\"hi\\""}]');
console.log("uni.text:" + uni[0].text);
console.log("uni.newline:" + uni[0].newline);
console.log("uni.tab:" + uni[0].tab);
console.log("uni.quote:" + uni[0].quote);

// 6. Number edge cases
const nums = JSON.parse('[0,-1,1.5e10,-2.5e-3,0.001,1000000]') as number[];
console.log("nums.length:" + nums.length);
for (let i = 0; i < nums.length; i++) console.log("nums[" + i + "]:" + nums[i]);

// 7. Empty string / whitespace-in-value handling
const ws = JSON.parse('[{"empty":"","space":" ","tab":"\\t"}]');
console.log("ws.empty.len:" + (ws[0].empty as string).length);
console.log("ws.space:'" + ws[0].space + "'");

// 8. Array of arrays (nested structure)
const nested = JSON.parse('[[1,2],[3,4],[5,6]]') as number[][];
console.log("nested.length:" + nested.length);
console.log("nested[0].length:" + nested[0].length);
console.log("nested[1][1]:" + nested[1][1]);
console.log("nested[2][0]:" + nested[2][0]);

// 9. Large-ish array (100 elements) with property access
const items: number[] = [];
for (let i = 0; i < 100; i++) items.push(i * 7);
const big = JSON.parse(JSON.stringify(items)) as number[];
let bigSum = 0;
for (let i = 0; i < big.length; i++) bigSum += big[i];
console.log("big.sum:" + bigSum);
console.log("big[50]:" + big[50]);

// 10. Roundtrip: parse → stringify → reparse (must be identical)
const original = JSON.parse('[{"k":1,"v":"one"},{"k":2,"v":"two"}]');
const restr = JSON.stringify(original);
const reparsed = JSON.parse(restr);
console.log("reparsed.length:" + reparsed.length);
console.log("reparsed[1].v:" + reparsed[1].v);

// 11. Negative zero and special numbers that JSON represents as `null`
// (Infinity / NaN don't survive JSON); this covers the allowed shapes.
const edge = JSON.parse('[null,null,0,-0]');
console.log("edge.len:" + edge.length);
console.log("edge[0]:" + edge[0]);
console.log("edge[2]:" + edge[2]);
console.log("edge[3]:" + edge[3]);
