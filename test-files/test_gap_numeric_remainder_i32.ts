let sum = 0;
for (let i = 0; i < 10000; i++) {
    sum = sum + (i % 1000);
}
console.log("positive-loop:" + sum);

let sawNegativeZero = false;
for (let i = -6; i <= 0; i++) {
    const value = i % 3;
    if (Object.is(value, -0)) {
        sawNegativeZero = true;
    }
}
console.log("negative-loop-zero:" + sawNegativeZero);

console.log("fractional:" + (5.5 % 2));
console.log("nan:" + (NaN % 2));
console.log("infinity:" + (Infinity % 2));
console.log("zero-divisor:" + (5 % 0));
console.log("infinite-divisor:" + (5 % Infinity));
console.log("negative-zero:" + Object.is(-0 % 3, -0));

let dynamicZero = 0;
console.log("dynamic-zero-divisor:" + (5 % dynamicZero));
console.log("negative-divisor:" + (5 % -2));
console.log("min-i32-negative-one:" + Object.is(-2147483648 % -1, -0));
console.log("wide-safe-integer:" + (9007199254740991 % 97));
