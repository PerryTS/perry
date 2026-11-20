// Test export * from "./utils"

import { square, double } from "./all_utils";

let sq = square(5);
console.log(sq); // Should print 25

let dbl = double(7);
console.log(dbl); // Should print 14
