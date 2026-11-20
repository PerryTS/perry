// Regression for #9143: BigInt operands bound by for-of must stay tagged
// through nested arithmetic. The add previously treated the left subtree as
// a Number and silently discarded the remainder term.

for (const a of [123456789012345678901234567890n]) {
  for (const d of [1000000007n]) {
    console.log("identity", (a / d) * d + (a % d));
    console.log("quotient", a / d);
    console.log("remainder", a % d);
    console.log("matches", (a / d) * d + (a % d) === a);
  }
}

const plainA = 123456789012345678901234567890n;
const plainD = 1000000007n;
console.log("plain", (plainA / plainD) * plainD + (plainA % plainD));
