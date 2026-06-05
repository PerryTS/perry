function errorName(fn: () => unknown): string {
  try {
    fn();
    return "none";
  } catch (err) {
    return (err as Error).name;
  }
}

const nowFn = Date.now;
const parseFn = Date.parse;
const utcFn = Date.UTC;

console.log("typeof:", typeof Date.now, typeof Date.parse, typeof Date.UTC);
console.log("names:", Date.now.name, Date.parse.name, Date.UTC.name);
console.log("lengths:", Date.now.length, Date.parse.length, Date.UTC.length);

const before = Date.now();
const nowValue = nowFn();
const after = Date.now();
console.log("now detached:", typeof nowValue, nowValue >= before && nowValue <= after);

console.log(
  "parse detached:",
  parseFn("1970-01-01T00:00:00.000Z"),
  Number.isNaN(parseFn("not a date")),
);
console.log("utc detached:", utcFn(1970, 0, 1), utcFn(2000, 1, 29, 12, 34, 56, 789));
console.log(
  "utc arity defaults:",
  utcFn(1970),
  Number.isNaN(utcFn()),
  Number.isNaN(utcFn(1970, undefined)),
);

const funcs = [Date.now, Date.parse, Date.UTC];
console.log(
  "array calls:",
  typeof funcs[0](),
  funcs[1]("1970-01-01T00:00:00.000Z"),
  funcs[2](1970, 0, 1),
);

const holder = { utc: Date.UTC };
console.log("object value:", typeof holder.utc, holder.utc(1970, 0, 2));

console.log(
  "direct calls:",
  typeof Date.now(),
  Date.parse("1970-01-01T00:00:00.000Z"),
  Date.UTC(1970, 0, 1),
);
