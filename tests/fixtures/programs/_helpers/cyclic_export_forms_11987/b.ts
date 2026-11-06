import * as a from "./a.ts";
import { d1, d2, fe, K, L, renamed } from "./a.ts";

function t(f: () => unknown): string {
  try {
    return String(f());
  } catch (error: any) {
    return error.constructor.name;
  }
}

export function read(): string {
  return [
    t(() => K),
    t(() => L),
    t(() => d1),
    t(() => d2),
    t(() => fe()),
    t(() => renamed),
    t(() => a.K),
    t(() => a.renamed),
  ].join(",");
}
