import { deserialize, serialize } from "node:v8";

const source: any = {
  date: new Date("2020-01-02T03:04:05.000Z"),
  regexp: /a+b/giu,
};
const result: any = deserialize(serialize(source));

console.log("date:", result.date instanceof Date, result.date.toISOString());
console.log(
  "regexp:",
  result.regexp instanceof RegExp,
  result.regexp.source,
  result.regexp.flags,
);
