// parity-env: PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_VERIFY_EVACUATION=1

function codeUnits(value: string): string {
  const units: string[] = [];
  for (let i = 0; i < value.length; i++) {
    units.push(value.charCodeAt(i).toString(16));
  }
  return units.join(",");
}

function dump(label: string, value: string): void {
  console.log(label, value.length, JSON.stringify(value), codeUnits(value));
}

const high = String.fromCharCode(0xd83d);
const low = String.fromCharCode(0xde00);
const astral = "😀";

const holes = new Array(5);
holes[1] = "";
holes[2] = high;
holes[3] = low;
holes[4] = astral;
dump("join-empty-holes", holes.join(""));
dump("join-separated-lone", [high, low].join("|"));
dump("join-astral", ["a", astral, "b"].join("—"));

let joinAllocations = 0;
function allocatingElement(text: string) {
  return {
    toString() {
      const scratch: string[] = [];
      for (let i = 0; i < 64; i++) scratch.push(text + i);
      joinAllocations += scratch.length;
      return text;
    },
  };
}
dump(
  "join-allocating-tostring",
  [allocatingElement("left"), allocatingElement(astral), allocatingElement("right")].join("::"),
);
console.log("join-allocations", joinAllocations);

dump("repeat-ascii", "ab".repeat(7));
dump("repeat-lone", high.repeat(3));
dump("repeat-boundary", (low + high).repeat(2));
dump("pad-start", low.padStart(2, high));
dump("pad-end", high.padEnd(2, low));
dump("pad-cycle", "x".padStart(6, astral + "a"));

dump("replace-literal", "abcabc".replaceAll("abc", "$`<$&>$'"));
dump("replace-empty-astral", astral.replaceAll("", "|"));
dump("replace-empty-boundary", low.replace("", high));
dump("replace-literal-boundary", (high + "X").replace("X", low));
dump("replace-regex-boundary", (high + "X").replace(/X/, low));
dump("replace-regex-no-match", (high + "X").replace(/Z/, low));
dump(
  "replace-regex",
  "John Smith; Ada Lovelace".replace(/(?<first>\w+) (?<last>\w+)/g, "$<last>, $<first>"),
);
dump("replace-fancy", "$5 and $10".replace(/(?<=\$)(?<n>\d+)/g, "[$<n>]"));

let callbackAllocations = 0;
const callbackResult = "a1b2c3".replaceAll(/\d/g, function (match, offset, whole) {
  const scratch: string[] = [];
  for (let i = 0; i < 64; i++) scratch.push(whole + match + i);
  callbackAllocations += scratch.length;
  return "[" + match + ":" + offset + "]";
});
dump("replace-callback", callbackResult);
console.log("replace-callback-allocations", callbackAllocations);
