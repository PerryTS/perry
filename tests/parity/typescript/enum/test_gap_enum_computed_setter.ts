// A reverse mapping consumes the forward assignment result, even when an
// inherited setter intercepts that store and leaves no own forward property.
// DEC 250: Object.prototype takes no new key in a compiled program, so the setter is refused and the forward store is an own property.
// tests/parity/expected holds Perry's output.
function locked(f: () => void) { try { f(); } catch (e) { console.log("DEC 250: " + (e as Error).message); } }
let input = 0;
let seen = 0;
locked(() => { Object.defineProperty(Object.prototype, "Captured", {
    configurable: true,
    set(value: number) { seen = value; },
}); });
enum E { Captured = input + 1.5 }
console.log(seen, E["1.5"], Object.keys(E).join(","));
locked(() => { delete (Object.prototype as any).Captured; });
