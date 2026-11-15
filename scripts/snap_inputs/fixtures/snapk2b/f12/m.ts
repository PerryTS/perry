// A promise job queued at module scope is still pending when n.ts starts.
Promise.resolve().then(() => { (globalThis as any).m = 1; });
export {};
