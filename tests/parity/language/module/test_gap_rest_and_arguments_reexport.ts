import { fetch } from "./fixtures/rest_and_arguments_export/star.ts";
const m = new Map<string, number>([["a", 1]]);
console.log(fetch(m, "a"));
console.log(fetch(m, "a", 2));
