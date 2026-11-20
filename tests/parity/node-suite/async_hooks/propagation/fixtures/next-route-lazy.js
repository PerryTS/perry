export function checksum(iterations) {
  let value = 0x811c9dc5;
  for (let index = 0; index < iterations; index += 1) {
    value = Math.imul(value ^ index, 0x01000193) >>> 0;
  }
  return value;
}
