// #8433: String.prototype.concat should coerce arguments left-to-right, keep
// every coerced part alive across re-entrant coercions, and use the N-way
// concat helper for the common <=32-part case.

const order: string[] = [];

function ordered(label: string): any {
  order.push("eval-" + label);
  return {
    toString(): string {
      order.push("coerce-" + label);
      // Allocate through concat while the outer call's receiver and earlier
      // coerced arguments are live.
      let churn = "";
      for (let i = 0; i < 40; i++) {
        churn = churn.concat(label, String(i), ":");
      }
      return label + churn.length;
    },
  };
}

const mixed = "start:".concat(
  undefined as any,
  ":",
  true as any,
  ":",
  false as any,
  ":",
  ordered("A"),
  ":",
  ordered("B"),
);
console.log(mixed);
console.log(order.join(","));

// The join between these two WTF-8 parts must canonicalize to one scalar.
const high = String.fromCharCode(0xd83d);
const low = String.fromCharCode(0xde00);
const joined = "<".concat(high, low, ">");
console.log(joined === "<😀>", joined.length, joined.charCodeAt(1), joined.charCodeAt(2));

// Receiver + 35 arguments exceeds the runtime's 32-part chain cap. The first
// 31 arguments fuse; the rooted pairwise tail must preserve all remaining
// parts and their order.
console.log(
  "many:".concat(
    "00", "01", "02", "03", "04", "05", "06", "07", "08", "09",
    "10", "11", "12", "13", "14", "15", "16", "17", "18", "19",
    "20", "21", "22", "23", "24", "25", "26", "27", "28", "29",
    "30", "31", "32", "33", "34",
  ),
);
