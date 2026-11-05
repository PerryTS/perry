// Main module that imports from math (which imports from utils)

import { add, multiply, squareAndDouble } from "./math";

let result = add(3, 4);
console.log(result); // Should print 7

let product = multiply(5, 6);
console.log(product); // Should print 30

// squareAndDouble(3) = double(square(3)) = double(9) = 18
let complex = squareAndDouble(3);
console.log(complex); // Should print 18
