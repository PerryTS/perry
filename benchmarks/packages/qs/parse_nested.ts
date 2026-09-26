// qs: parse nested bracket query strings (objects + arrays).
import qs from "qs";
import { iters, header, fnv, hex, FNV_SEED } from "../_lib/bench.ts";

const it = iters(20000, 1000);
header("qs/parse_nested", "qs", it);
const queries: string[] = [];
for (let k = 0; k < 16; k++) {
  queries.push("user[name]=alice" + k + "&user[roles][]=admin&user[roles][]=dev" +
    "&filter[age][gte]=" + (18 + k) + "&filter[age][lte]=65&sort=-created&page[size]=" + (10 + k) +
    "&page[number]=" + k + "&q=hello%20world%26more&tags[0]=a&tags[1]=b&tags[2]=c" + k);
}

function op(i: number, h: number): number {
  const o: any = qs.parse(queries[i & 15]);
  return fnv(h, o.user.name + o.user.roles.join(",") + o.filter.age.gte + o.page.size + o.q + o.tags[2]);
}
let h = FNV_SEED;
for (let i = 0; i < it.warm; i++) h = op(i, h);
h = FNV_SEED;
for (let i = 0; i < it.n; i++) h = op(i, h);
console.log("checksum " + hex(h));
