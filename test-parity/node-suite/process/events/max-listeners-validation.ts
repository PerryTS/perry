function probe(label: string, fn: () => unknown): void {
  try {
    console.log(label, "OK", fn());
  } catch (err: any) {
    console.log(label, "THROW", err.name, err.code, err.message.split("\n")[0]);
  }
}

for (const value of [0, 1, 1.5, Infinity]) {
  probe(`max ${String(value)}`, () => {
    const ret = process.setMaxListeners(value);
    return `${ret === process},${String(process.getMaxListeners())}`;
  });
}

for (const value of [-1, NaN, "5", null, undefined]) {
  probe(`max ${String(value)}`, () => process.setMaxListeners(value as any));
}
