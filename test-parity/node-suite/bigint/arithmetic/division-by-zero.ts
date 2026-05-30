function probe(label: string, fn: () => bigint): void {
  try {
    console.log(label, "ok", String(fn()));
  } catch (e) {
    const err = e as Error;
    console.log(label, "throw", err.name, err.message, e instanceof RangeError);
  }
}

const zero = BigInt(0);

probe("div literal zero", () => 1n / 0n);
probe("mod literal zero", () => 1n % 0n);
probe("div variable zero", () => 123n / zero);
probe("mod variable zero", () => 123n % zero);
probe("div nonzero", () => 7n / 2n);
probe("mod nonzero", () => -7n % 2n);
