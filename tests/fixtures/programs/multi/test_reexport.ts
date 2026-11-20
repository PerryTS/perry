// Test re-exports by importing from index

import { add, square } from "./index";

let sum = add(10, 20);
console.log(sum); // Should print 30

let sq = square(7);
console.log(sq); // Should print 49
