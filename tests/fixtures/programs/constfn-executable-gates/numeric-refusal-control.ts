// Keep descriptor syntax outside the executable's loop module: the runtime
// branch, rather than frontend syntax suppression, must reject admission.
export function countedPairAccessor(pair: any): () => string {
    let current = pair.a, gets = 0, sets = 0, coercions = 0;
    const boxed = { valueOf: () => { coercions++; return current; } };
    Object.defineProperty(Object.getPrototypeOf(pair), "a", {
        value: 0, writable: true, configurable: true,
    });
    Object.defineProperty(pair, "a", {
        get: () => { gets++; return boxed; },
        set: (value: number) => { sets++; current = value; },
        enumerable: true, configurable: true,
    });
    return () => `${current} ${gets} ${sets} ${coercions}`;
}

export function countedIndexedAccessor(entities: number[]): () => number {
    const original = entities[0];
    let gets = 0;
    Object.defineProperty(entities, "0", {
        get: () => { gets++; return original; },
        enumerable: true, configurable: true,
    });
    return () => gets;
}

export function countedCellAccessor(cell: any): () => string {
    let current = cell.a, gets = 0, sets = 0;
    Object.defineProperty(cell, "a", {
        get: () => { gets++; return current; },
        set: (value: number) => { sets++; current = value; },
        enumerable: true, configurable: true,
    });
    return () => `${current} ${gets} ${sets}`;
}
