// Gap test for #2754 / #2907 / #2908 — BigInt() coercion + operator semantics.
// Each case prints err.name + ": " + err.message (or "ok <value>") so the
// output is byte-comparable against `node --experimental-strip-types`.

function run(label: string, fn: () => unknown): void {
  try {
    console.log(label + " ok " + String(fn()));
  } catch (e) {
    const err = e as Error;
    console.log(label + " throw " + err.name + ": " + err.message);
  }
}

// ---- BigInt() coercion (#2754 / #2907) ----
run("BigInt()", () => BigInt());
run("BigInt(undefined)", () => BigInt(undefined));
run("BigInt(null)", () => BigInt(null as unknown as number));
run("BigInt(1.5)", () => BigInt(1.5));
run("BigInt(NaN)", () => BigInt(NaN));
run("BigInt(Infinity)", () => BigInt(Infinity));
run("BigInt(-Infinity)", () => BigInt(-Infinity));
run("BigInt(42)", () => BigInt(42));
run("BigInt(true)", () => BigInt(true));
run("BigInt(false)", () => BigInt(false));
run('BigInt("0x10")', () => BigInt("0x10"));
run('BigInt("0o17")', () => BigInt("0o17"));
run('BigInt("0b101")', () => BigInt("0b101"));
run('BigInt("  42  ")', () => BigInt("  42  "));
run('BigInt("")', () => BigInt(""));
run('BigInt("bad")', () => BigInt("bad"));
run('BigInt("12abc34")', () => BigInt("12abc34"));

// ---- Operator semantics (#2908) ----
run("1n + 1", () => (1n as bigint) + (1 as unknown as bigint));
run("1n - 1", () => (1n as bigint) - (1 as unknown as bigint));
run("1n * 2n", () => 1n * 2n);
run("5n / 2n", () => 5n / 2n);
run("5n % 2n", () => 5n % 2n);
run("2n ** 3n", () => 2n ** 3n);
run("2n ** -1n", () => 2n ** -1n);
run("5n / 0n", () => 5n / 0n);
run("5n % 0n", () => 5n % 0n);
run("1n >>> 0n", () => (1n as bigint) >>> (0n as unknown as number));
run("1n << -1n", () => 1n << -1n);
run("8n >> -1n", () => 8n >> -1n);
run("1n << 4n", () => 1n << 4n);
run("256n >> 4n", () => 256n >> 4n);
