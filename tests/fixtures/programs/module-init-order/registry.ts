const KEYS: number[] = [];
const VALUES: string[] = [];

export function register(oid: number, codec: string): void {
    KEYS.push(oid);
    VALUES.push(codec);
}

export function lookup(oid: number): string {
    const idx = KEYS.indexOf(oid);
    return idx < 0 ? "MISSING" : VALUES[idx];
}

export function count(): number {
    return KEYS.length;
}
