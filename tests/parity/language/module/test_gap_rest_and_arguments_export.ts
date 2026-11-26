import { fetch } from "./fixtures/rest_and_arguments_export/lib.ts";
const m = new Map<string, number>([["a", 1]]);
console.log(fetch(m, "a"));
console.log(fetch(m, "a", 2));
class K { static readonly X: unknown[] = [fetch(m, "a"), fetch(m, "b", 3)]; }
console.log(K.X.join(" | "));
