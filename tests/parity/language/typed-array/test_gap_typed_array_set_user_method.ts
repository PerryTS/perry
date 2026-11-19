// `ta.set(...)` at a site the compiler folds reads `set` like any method
// call: a user `set` on the receiver's chain wins over the builtin.

// A user `set` on the receiver's chain wins over %TypedArray%.prototype.set.
const plain = new Float32Array(4);
plain.set([1, 2], 1);
console.log("builtin", Array.from(plain).join(","));

const own = new Float32Array(4);
const seen: unknown[] = [];
Object.setPrototypeOf(own, {
  set(...args: unknown[]) { seen.push(args.length, args[0]); },
});
own.set([9, 9]);
own.set([8], 2);
console.log("relinked receiver", seen.join(","), own[0], own[1]);

(Float64Array.prototype as any).set = function (this: Float64Array, src: ArrayLike<number>) {
  seen.push("f64 set", src.length, arguments.length);
};
const f64 = new Float64Array(2);
f64.set([5, 6]);
console.log("kind prototype set", seen.slice(-3).join(","), f64[0]);
delete (Float64Array.prototype as any).set;
f64.set([5, 6]);
console.log("restored", f64[0], f64[1]);

const arr: number[] = [1, 2, 3];
try {
  (arr as any).set([4]);
  console.log("array set ran");
} catch (e) {
  console.log("array set", (e as Error).constructor.name);
}
