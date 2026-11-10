function show(label: string, fn: () => unknown) {
  console.log(`${label}:`, JSON.stringify(fn()));
}

show("local omitted start", () => {
  const a = [1, 2, 3, 4];
  const r = a.splice();
  return [r, a];
});

show("local start only", () => {
  const a = [1, 2, 3, 4];
  const r = a.splice(1);
  return [r, a];
});

show("local explicit undefined delete", () => {
  const a = [1, 2, 3, 4];
  const r = a.splice(1, undefined as any, 9);
  return [r, a];
});

show("local delete two insert", () => {
  const a = [1, 2, 3, 4];
  const r = a.splice(1, 2, 9);
  return [r, a];
});

show("chained omitted start", () => {
  const box = { a: [1, 2, 3, 4] };
  const r = box.a.splice();
  return [r, box.a];
});

show("dynamic omitted start", () => {
  const a: any = [1, 2, 3, 4];
  const r = a.splice();
  return [r, a];
});

show("dynamic start only", () => {
  const a: any = [1, 2, 3, 4];
  const r = a.splice(1);
  return [r, a];
});

show("dynamic explicit undefined delete", () => {
  const a: any = [1, 2, 3, 4];
  const r = a.splice(1, undefined, 9);
  return [r, a];
});
