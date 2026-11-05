function fill7221(
  values: Float64Array,
  nodes: number,
  dirty: number,
  frame: number,
  initialChecksum: number,
): number {
  let checksum = initialChecksum;
  for (let row = 0; row < dirty; row++) {
    const index = nodes - dirty + row;
    const value = frame * 0.001 + row * 0.01;
    values[index] = value;
    checksum = (checksum + index + value * 1000) % 4_294_967_296;
  }
  return checksum;
}

const nodes7221 = 128;
const dirty7221 = 32;
const frames7221 = 100;

const helperValues7221 = new Float64Array(nodes7221);
let helperChecksum7221 = 0;
for (let frame = 0; frame < frames7221; frame++) {
  helperChecksum7221 = fill7221(
    helperValues7221,
    nodes7221,
    dirty7221,
    frame,
    helperChecksum7221,
  );
}

const inlineValues7221 = new Float64Array(nodes7221);
let inlineChecksum7221 = 0;
for (let frame = 0; frame < frames7221; frame++) {
  for (let row = 0; row < dirty7221; row++) {
    const index = nodes7221 - dirty7221 + row;
    const value = frame * 0.001 + row * 0.01;
    inlineValues7221[index] = value;
    inlineChecksum7221 =
      (inlineChecksum7221 + index + value * 1000) % 4_294_967_296;
  }
}

if (helperChecksum7221 !== inlineChecksum7221) {
  throw new Error(
    `helper/inline checksum mismatch: ${helperChecksum7221} !== ${inlineChecksum7221}`,
  );
}

console.log("issue7221:", helperChecksum7221, helperValues7221[nodes7221 - 1]);
