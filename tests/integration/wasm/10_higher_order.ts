// Array higher-order methods
const nums = [1, 2, 3, 4, 5];
const doubled = nums.map((x: number) => x * 2);
console.log(doubled.join(","));

const evens = nums.filter((x: number) => x % 2 === 0);
console.log(evens.join(","));

const sum = nums.reduce((acc: number, x: number) => acc + x, 0);
console.log(sum);

const found = nums.find((x: number) => x > 3);
console.log(found);

const hasThree = nums.some((x: number) => x === 3);
console.log(hasThree);

const allPositive = nums.every((x: number) => x > 0);
console.log(allPositive);
