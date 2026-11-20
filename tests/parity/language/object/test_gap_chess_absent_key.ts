// A read of a key no layout of the receiver set has, on a fixed layout whose chain is intrinsic: undefined.
interface Opts { level?: number; maxDepth?: number }
function search(options: Opts = {}): number { return (options.level ?? 5) * 10 + (options.maxDepth ?? 3); }
console.log(search({ level: 2 }), search());
