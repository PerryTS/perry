// #9784: logical array length must not prove backing-store capacity.

const boundarySlots = 1000000;
const boundary: number[] = new Array(boundarySlots);
for (let i = 0; i < boundarySlots; i++) boundary[i] = i;
let boundaryWrong = 0;
let boundaryChecksum = 0;
for (let i = 0; i < boundarySlots; i++) {
  if (boundary[i] !== i) boundaryWrong++;
  boundaryChecksum = (boundaryChecksum + boundary[i]) % 1000000007;
}
console.log(boundarySlots, boundaryWrong, boundaryChecksum, boundary[0], boundary[boundarySlots - 1]);

const aboveSlots = 1000001;
const above: number[] = new Array(aboveSlots);
for (let i = 0; i < aboveSlots; i++) above[i] = i;
let aboveWrong = 0;
let aboveChecksum = 0;
for (let i = 0; i < aboveSlots; i++) {
  if (above[i] !== i) aboveWrong++;
  aboveChecksum = (aboveChecksum + above[i]) % 1000000007;
}
console.log(aboveSlots, aboveWrong, aboveChecksum, above[0], above[aboveSlots - 1]);

const largerSlots = 1200000;
const larger: number[] = new Array(largerSlots);
for (let i = 0; i < largerSlots; i++) larger[i] = i;
let largerWrong = 0;
let largerChecksum = 0;
for (let i = 0; i < largerSlots; i++) {
  if (larger[i] !== i) largerWrong++;
  largerChecksum = (largerChecksum + larger[i]) % 1000000007;
}
console.log(largerSlots, largerWrong, largerChecksum, larger[0], larger[largerSlots - 1]);

// A literal allocation inside a function also supplies a static length proof.
function literalLocal(): void {
  const values: number[] = new Array(1000001);
  for (let i = 0; i < 1000001; i++) values[i] = i;
  let wrong = 0;
  for (let i = 0; i < 1000001; i++) if (values[i] !== i) wrong++;
  console.log("literal", wrong, values[0], values[1000000]);
}
literalLocal();
