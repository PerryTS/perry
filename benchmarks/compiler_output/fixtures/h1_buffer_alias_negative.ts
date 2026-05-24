const SIZE = 32;

function aliasLocal(): number {
  const owned = Buffer.alloc(SIZE);
  const alias = owned;
  let total = 0;
  alias_local:
  for (let i = 0; i < owned.length; i++) {
    total = (total + alias[i]) | 0;
  }
  return total;
}

function reassignment(): number {
  let buf = Buffer.alloc(SIZE);
  buf = Buffer.alloc(SIZE / 2);
  let total = 0;
  reassignment_region:
  for (let i = 0; i < SIZE / 2; i++) {
    total = (total + buf[i]) | 0;
  }
  return total;
}

function passToUnknown(value: any): number {
  return value ? 1 : 0;
}

function unknownCallEscape(): number {
  const owned = Buffer.alloc(SIZE);
  const escaped = passToUnknown(owned);
  let total = escaped | 0;
  unknown_call_escape:
  for (let i = 0; i < owned.length; i++) {
    total = (total + owned[i]) | 0;
  }
  return total;
}

function closureCapture(): number {
  const owned = Buffer.alloc(SIZE);
  const read = (i: number) => owned[i];
  let total = 0;
  closure_capture:
  for (let i = 0; i < owned.length; i++) {
    total = (total + read(i)) | 0;
  }
  return total;
}

function sharedBacking(): number {
  const owned = Buffer.alloc(SIZE);
  const view = owned.slice(0, SIZE / 2);
  let total = 0;
  shared_backing:
  for (let i = 0; i < view.length; i++) {
    total = (total + view[i]) | 0;
  }
  return total;
}

function lengthMismatch(src: Buffer, dst: Buffer): number {
  let total = 0;
  length_mismatch:
  for (let i = 0; i < src.length; i++) {
    dst[i] = (src[i] + 1) & 255;
    total = (total + dst[i]) | 0;
  }
  return total;
}

const shortSrc = Buffer.alloc(SIZE / 2);
const shortDst = Buffer.alloc(SIZE / 4);

console.log(
  "h1_buffer_alias_negative:" +
    ((
      aliasLocal() +
      reassignment() +
      unknownCallEscape() +
      closureCapture() +
      sharedBacking() +
      lengthMismatch(shortSrc, shortDst)
    ) |
      0),
);
