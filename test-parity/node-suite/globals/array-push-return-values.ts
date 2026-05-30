function log(label: string, fn: () => unknown) {
  try {
    console.log(label, JSON.stringify(fn()));
  } catch (err: any) {
    console.log(label, "throw", err.name, err.message);
  }
}

log("local zero", () => {
  const a = [1, 2];
  const result = a.push();
  return [result, a];
});

log("local multi", () => {
  const a = [1];
  const result = a.push(2, 3);
  return [result, a];
});

log("computed dynamic multi", () => {
  const a: any = [1];
  const method = "push";
  const result = a[method](2, 3, 4);
  return [result, a];
});

log("spread", () => {
  const a = [1];
  const result = a.push(...[2, 3]);
  return [result, a];
});
