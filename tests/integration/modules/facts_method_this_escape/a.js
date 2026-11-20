export const lit = { n: 1, run() { return globalThis.zz(this); } };
export function show() { return typeof lit.n + ":" + (lit.n + 1); }
