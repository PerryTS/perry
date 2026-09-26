// date-fns: differenceIn*, startOf/endOf, eachDayOfInterval, isWithinInterval.
import { differenceInCalendarDays, differenceInHours, startOfWeek, endOfMonth,
  eachDayOfInterval, isWithinInterval, addHours, formatISO } from "date-fns";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

const it = iters(10000, 500);
header("date-fns/diff_interval", "date-fns", it);
const base = new Date(Date.UTC(2024, 0, 1));

function op(i: number, h: number): number {
  const a = addHours(base, i % 1000);
  const b = endOfMonth(addHours(a, 24 * ((i * 7) % 90)));
  const days = eachDayOfInterval({ start: a, end: addHours(a, 24 * (i % 10)) }).length;
  const inside = isWithinInterval(addHours(a, 5), { start: a, end: b });
  return fnv(h, differenceInCalendarDays(b, a) + ":" + differenceInHours(b, a) + ":" + days + ":" +
    inside + ":" + formatISO(startOfWeek(a)));
}
let h = FNV_SEED;
for (let i = 0; i < it.warm; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < it.n; i++) h = op(i, h);
console.log("checksum " + hex(h));
