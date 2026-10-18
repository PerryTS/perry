// A reverse mapping consumes the forward assignment result, even when an
// inherited setter intercepts that store and leaves no own forward property.
let input = 0;
let seen = 0;
Object.defineProperty(Object.prototype, "Captured", {
    configurable: true,
    set(value: number) { seen = value; },
});
enum E { Captured = input + 1.5 }
console.log(seen, E["1.5"], Object.keys(E).join(","));
delete (Object.prototype as any).Captured;
