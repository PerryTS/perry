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
  let total = read(0) | 0;
  closure_capture:
  for (let i = 0; i < owned.length; i++) {
    total = (total + owned[i]) | 0;
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

function mutatedForIndex(buf: Buffer): number {
  let total = 0;
  mutated_for_index:
  for (let i = 0; i < 8; i++) {
    i = 16;
    total = (total + buf[i]) | 0;
  }
  return total;
}

function mutatedWhileIndex(buf: Buffer): number {
  let total = 0;
  let i = 0;
  mutated_while_index:
  while (i < 8) {
    i = 16;
    total = (total + buf[i]) | 0;
    i++;
  }
  return total;
}

function staleNativeAlias(): number {
  const buf = Buffer.alloc(8);
  stale_native_alias:
  for (let i = 0; i < buf.length; i++) {
    let j = i | 0;
    j = 16;
    buf[j] = 1;
  }
  return 0;
}

function staleAllocationLength(): number {
  let n = 8;
  const buf = Buffer.alloc(n);
  n = 16;
  stale_allocation_length:
  for (let i = 0; i < n; i++) {
    buf[i] = 1;
  }
  return 0;
}

function arrayBufferViews(): number {
  const ab = new ArrayBuffer(8);
  const a = new Uint8Array(ab);
  const b = new Uint8Array(ab);
  let total = 0;
  array_buffer_views:
  for (let i = 0; i < a.length; i++) {
    a[i] = (i + 1) & 255;
    b[i] = (b[i] + a[i]) & 255;
    total = (total + b[i]) | 0;
  }
  return total;
}

const shortSrc = Buffer.alloc(SIZE / 2);
const shortDst = Buffer.alloc(SIZE / 4);
const mutationBuf = Buffer.alloc(8);

console.log(
  "h1_buffer_alias_negative:" +
    ((
      aliasLocal() +
      reassignment() +
      unknownCallEscape() +
      closureCapture() +
      sharedBacking() +
      lengthMismatch(shortSrc, shortDst) +
      mutatedForIndex(mutationBuf) +
      mutatedWhileIndex(mutationBuf) +
      staleNativeAlias() +
      staleAllocationLength() +
      arrayBufferViews()
    ) |
      0),
);
