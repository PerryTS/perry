// Issue #4987: `import process from 'node:process'` and `globalThis.process`
// must expose the same fully-populated process object as the bare `process`
// identifier (env/stdout/stderr/stdin/platform/arch/argv/pid/ppid/version/
// versions were undefined on the import + globalThis forms).
// Expected: byte-identical to node --experimental-strip-types
import proc from 'node:process';

// --- minimal repro from the issue ---
console.log(typeof proc.env, typeof proc.stdout, typeof proc.platform);
console.log(typeof process.env);

// --- full member sweep on the default import ---
console.log(typeof proc.env === 'object'); // true
console.log(typeof proc.stdout === 'object'); // true
console.log(typeof proc.stderr === 'object'); // true
console.log(typeof proc.stdin === 'object'); // true
console.log(typeof proc.platform === 'string'); // true
console.log(typeof proc.arch === 'string'); // true
console.log(typeof proc.pid === 'number'); // true
console.log(typeof proc.ppid === 'number'); // true
console.log(typeof proc.version === 'string'); // true
console.log(typeof proc.versions === 'object'); // true
console.log(Array.isArray(proc.argv)); // true
console.log(typeof proc.cwd === 'function'); // true

// --- globalThis.process ---
console.log(typeof globalThis.process.env === 'object'); // true
console.log(typeof globalThis.process.stdout === 'object'); // true
console.log(typeof globalThis.process.platform === 'string'); // true

// --- values agree with the bare identifier ---
console.log(proc.platform === process.platform); // true
console.log(proc.arch === process.arch); // true
console.log(proc.pid === process.pid); // true
console.log(proc.version === process.version); // true
console.log(typeof proc.env.PATH === 'string'); // true
console.log(proc.env.PATH === process.env.PATH); // true

// --- terminal-size shape (the ink #348 wall): destructure off the import ---
const { env, stdout, stderr } = proc;
console.log(typeof env, typeof stdout, typeof stderr); // object object object
// env member reads must not throw (this was the TypeError in terminal-size)
console.log(env.COLUMNS === undefined || typeof env.COLUMNS === 'string'); // true
console.log(typeof stdout.write === 'function'); // true
