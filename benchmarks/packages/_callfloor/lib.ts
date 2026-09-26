// Helper module for f12: an object built in one module, read in another.
export function make(n: number): { n: number; k: number } { return { n: n, k: 3 }; }
export function touch(o: { n: number; k: number }): void { if (o.n < 0) console.log("neg"); }
