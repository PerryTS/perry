// Kept in another module so descriptor syntax cannot suppress formation in
// numeric-loops.ts. The same executable chooses positive/refusal at runtime.
export function refuseNumericRoutes(pair: any, entities: number[]): void {
    // Installing a declared field descriptor on a class prototype retires the
    // classic whole-loop guard globally. Keep this in the separate module so
    // its syntax cannot suppress positive loop formation at compile time.
    Object.defineProperty(Object.getPrototypeOf(pair), "a", {
        value: 0, writable: true, configurable: true,
    });
    let current = pair.a;
    Object.defineProperty(pair, "a", {
        get: () => current,
        set: (value: number) => { current = value; },
        enumerable: true,
        configurable: true,
    });
    // The array still reads the identical value, but its descriptor flag must
    // refuse versioned indexed admission. No Array.prototype pollution needed.
    Object.defineProperty(entities, "0", {
        value: entities[0],
        writable: true,
        enumerable: true,
        configurable: true,
    });
}
