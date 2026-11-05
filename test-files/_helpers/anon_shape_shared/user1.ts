import { Query } from "./shape.ts";

const query = new Query();
export function build1(n: number) {
  const params = query.params(n * 1, "term1");
  return Object.keys(params).join(",") + ":" + params.limit + ":" + params.search;
}
