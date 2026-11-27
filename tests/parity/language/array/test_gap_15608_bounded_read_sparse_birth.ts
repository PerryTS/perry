// A function-local `new Array(k)` past the dense bound (1,000,000) has
// length `k` over an empty store. A loop's read of it is bounded by
// the length only, so the slot past the store reads a hole, never memory.

function noop(_i: number): void {}

function scan(k: number): string {
  const a = new Array(k);
  let defined = 0;
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== undefined) defined++;
  }
  return `scan ${k} ${defined} ${a.length}`;
}

function scanWithCall(): string {
  const a = new Array(2_000_000);
  let defined = 0;
  for (let i = 0; i < a.length; i++) {
    noop(i);
    if (a[i] !== undefined) defined++;
  }
  return `call ${defined}`;
}

function written(): string {
  const a = new Array(1_500_000);
  for (let i = 0; i < a.length; i += 9) a[i] = i & 7;
  let sum = 0;
  let holes = 0;
  for (let i = 0; i < a.length; i++) {
    const v = a[i];
    if (v === undefined) holes++;
    else sum += v;
  }
  return `written ${sum} ${holes}`;
}

function forOf(): string {
  const a = new Array(2_000_000);
  let holes = 0;
  for (const x of a) if (x === undefined) holes++;
  return `for-of ${holes}`;
}

function birth(): any[] { return new Array(2_000_000); }

function forOfCall(): string {
  let holes = 0;
  for (const x of birth()) {
    noop(0);
    if (x === undefined) holes++;
  }
  return `for-of call ${holes}`;
}

function reverse(): string {
  const a = new Array(2_000_000);
  let defined = 0;
  let i = a.length;
  while (i-- > 0) if (a[i] !== undefined) defined++;
  return `reverse ${defined}`;
}

function extended(): string {
  const a: number[] = [1, 2, 3];
  a.length = 2_000_000;
  let sum = 0;
  for (let i = 0; i < a.length; i++) {
    noop(i);
    if (a[i] !== undefined) sum += a[i];
  }
  return `extended ${sum}`;
}

console.log(scan(2_000_000));
console.log(scan(1_000_000));
console.log(scanWithCall());
console.log(written());
console.log(forOf());
console.log(forOfCall());
console.log(reverse());
console.log(extended());
