import { Query } from "./shape.ts";

const query = new Query();
export function build3(n: number) {
  const params = query.params(n * 3, "term3");
  return Object.keys(params).join(",") + ":" + params.limit + ":" + params.search;
}
