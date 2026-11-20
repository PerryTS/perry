function isNanValue(value: number) {
  return value !== value;
}

function show(label: string, fn: () => unknown) {
  console.log(`${label}:`, JSON.stringify(fn()));
}

show("local setFullYear invalid", () => {
  const d = new Date(NaN);
  const ret = d.setFullYear(2020);
  return [
    isNanValue(ret),
    isNanValue(d.getTime()),
    d.getFullYear(),
    d.getMonth(),
    d.getDate(),
    d.getHours(),
    d.getMinutes(),
    d.getSeconds(),
    d.getMilliseconds(),
  ];
});

show("utc setUTCFullYear invalid", () => {
  const d = new Date(NaN);
  const ret = d.setUTCFullYear(2020);
  return [isNanValue(ret), isNanValue(d.getTime()), d.toISOString()];
});

show("local setMonth stays invalid", () => {
  const d = new Date(NaN);
  const ret = d.setMonth(0);
  return [isNanValue(ret), isNanValue(d.getTime())];
});

show("utc setUTCMonth stays invalid", () => {
  const d = new Date(NaN);
  const ret = d.setUTCMonth(0);
  return [isNanValue(ret), isNanValue(d.getTime())];
});

show("local setDate stays invalid", () => {
  const d = new Date(NaN);
  const ret = d.setDate(1);
  return [isNanValue(ret), isNanValue(d.getTime())];
});

show("local setHours stays invalid", () => {
  const d = new Date(NaN);
  const ret = d.setHours(0);
  return [isNanValue(ret), isNanValue(d.getTime())];
});
