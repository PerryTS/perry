function fromBits(hi: number, lo: number): number {
  const buffer = new ArrayBuffer(8);
  const view = new DataView(buffer);
  view.setUint32(0, hi, false);
  view.setUint32(4, lo, false);
  return view.getFloat64(0, false);
}

function observableForms(value: number): string {
  return [
    String(value),
    `${value}`,
    value.toString(),
    value.toPrecision(),
    JSON.stringify(value),
    [value].join(","),
    "" + value,
    value + "",
  ].join("|");
}

const ties = [
  // The issue's exact tie, its adjacent doubles, and positive counterpart.
  fromBits(0xc0d92b80, 0x21ffffff),
  fromBits(0xc0d92b80, 0x22000000),
  fromBits(0xc0d92b80, 0x22000001),
  fromBits(0x40d92b80, 0x22000000),
  // Further exact ties found by the issue's seeded differential sweep.
  fromBits(0xc03cd941, 0x00000000),
  fromBits(0x40d14b6e, 0xda000000),
  fromBits(0x40c1b0ff, 0xd4000000),
];
for (let i = 0; i < ties.length; i++) {
  console.log("tie", i, observableForms(ties[i]));
}

const boundaries: [string, number][] = [
  ["large-scientific", 1e21],
  ["small-scientific", 1e-7],
  ["large-fixed", 1e20],
  ["small-fixed", 1e-6],
  ["max-value", Number.MAX_VALUE],
  ["min-normal", 2.2250738585072014e-308],
  ["min-value", Number.MIN_VALUE],
  ["epsilon", Number.EPSILON],
  ["below-fast-integer", 999999999999999],
  ["below-fast-fraction", 999999999999999.9],
  ["above-fast-integer", 1000000000000000],
  ["above-fast-fraction", 1000000000000000.1],
  ["negative-zero", -0],
  ["positive-infinity", Infinity],
  ["negative-infinity", -Infinity],
  ["nan", NaN],
];
for (let i = 0; i < boundaries.length; i++) {
  console.log(boundaries[i][0], observableForms(boundaries[i][1]));
}
