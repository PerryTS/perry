import { Query } from "./shape.ts";

const query = new Query();
export function build2(n: number) {
  const params = query.params(n * 2, "term2");
  return Object.keys(params).join(",") + ":" + params.limit + ":" + params.search;
}
