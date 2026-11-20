const positive: any = NaN;
const negative: any = Math.acosh(0);
const values: any[] = [positive, negative, 0, -0, 1, undefined, null, true, "NaN", {}, []];
for (const nan of [positive, negative]) {
  console.log(values.map(value => nan === value).join(","));
  console.log(values.map(value => value === nan).join(","));
  console.log([nan].indexOf(nan), [nan].includes(nan));
}
console.log(values.map(value => value === value).join(","));
console.log(values[2] === values[3], values[5] === values[6]);
