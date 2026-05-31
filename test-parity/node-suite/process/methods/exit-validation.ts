function probe(label: string, fn: () => unknown): void {
  try {
    console.log(label, "OK", fn());
  } catch (err: any) {
    console.log(label, "THROW", err.name, err.code, err.message.split("\n")[0]);
  }
}

probe("exit string fractional", () => process.exit("2.5" as any));
probe("exit string text", () => process.exit("abc" as any));
probe("exit boolean", () => process.exit(true as any));
probe("exit fractional", () => process.exit(1.9 as any));
probe("exit nan", () => process.exit(NaN as any));
probe("exit infinity", () => process.exit(Infinity as any));

process.exit("0" as any);
