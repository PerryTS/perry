// A namespace import's function called with spread arguments (#15079).
import * as H from "./namespace_spread_helper.ts";

const xs: number[] = [1, 2];
console.log(H.sum(...(xs as [number, number])));
console.log(H.sum(0, ...(xs.slice(1) as [number])));
