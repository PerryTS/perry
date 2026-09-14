// Exported bodies are the subjects of the machine-instruction census.
export function identity(x: any): any { return x; }
export function aliasOne(x: any): any { const y = x; return y; }
export function aliasThree(x: any): any { const a = x; const b = a; const c = b; return c; }
export function deadAssignments(x: any): any { let y = x; y = x; y = x; return x; }
export function aliasAcrossCall(x: any, visit: () => void): any { const y = x; visit(); return y; }
let counter: number = 0;
export function globalConstantWrite(): void { counter = 3; }
export function globalWrite(x: number): void { counter = x; }
export function callback(f: (x: number) => number, x: number): number { return f(x); }
