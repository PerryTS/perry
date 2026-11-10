// Test missing process features for Perry parity gap analysis
// Expected: all assertions pass under node --experimental-strip-types
// process is global, no import needed

// --- process.pid ---
console.log(typeof process.pid === 'number'); // true
console.log(process.pid > 0); // true

// --- process.ppid ---
console.log(typeof process.ppid === 'number'); // true
console.log(process.ppid > 0); // true

// --- process.version ---
console.log(typeof process.version === 'string'); // true
console.log(process.version.startsWith('v')); // true

// --- process.versions ---
console.log(typeof process.versions === 'object'); // true
console.log(typeof process.versions.node === 'string'); // true
console.log(typeof process.versions.v8 === 'string'); // true

// --- process.hrtime.bigint ---
const hr1 = process.hrtime.bigint();
console.log(typeof hr1 === 'bigint'); // true
console.log(hr1 > 0n); // true
// Ensure time progresses
const hr2 = process.hrtime.bigint();
console.log(hr2 >= hr1); // true

// --- process.nextTick ---
let nextTickCalled = false;
await new Promise<void>((resolve) => {
  process.nextTick(() => {
    nextTickCalled = true;
    resolve();
  });
});
console.log(nextTickCalled); // true

// --- process.on('exit') ---
let exitHandlerRegistered = false;
process.on('exit', () => {
  // This runs when the process exits; we just verify registration works
});
exitHandlerRegistered = true;
console.log(exitHandlerRegistered); // true

// --- process.chdir / process.cwd ---
const originalDir = process.cwd();
console.log(typeof originalDir === 'string'); // true
console.log(originalDir.length > 0); // true

process.chdir('/tmp');
console.log(process.cwd() === '/tmp' || process.cwd() === '/private/tmp'); // true (macOS resolves /tmp -> /private/tmp)

// Restore original directory
process.chdir(originalDir);
console.log(process.cwd() === originalDir); // true

// --- process.stdin, process.stdout, process.stderr ---
console.log(typeof process.stdin === 'object'); // true
console.log(typeof process.stdout === 'object'); // true
console.log(typeof process.stderr === 'object'); // true
console.log(process.stdin !== null); // true
console.log(process.stdout !== null); // true
console.log(process.stderr !== null); // true

// Verify stdout is writable
console.log(typeof process.stdout.write === 'function'); // true

// --- process.kill(pid, 0) --- signal check (0 means check if process exists)
let killCheck = false;
try {
  process.kill(process.pid, 0);
  killCheck = true;
} catch {
  killCheck = false;
}
console.log(killCheck); // true (own process exists)

// --- process.arch ---
console.log(typeof process.arch === 'string'); // true
console.log(process.arch.length > 0); // true

// --- process.platform ---
console.log(typeof process.platform === 'string'); // true
console.log(process.platform.length > 0); // true

// --- process.uptime ---
const uptime = process.uptime();
console.log(typeof uptime === 'number'); // true
console.log(uptime >= 0); // true

// --- process.memoryUsage ---
const mem = process.memoryUsage();
console.log(typeof mem === 'object'); // true
console.log(typeof mem.rss === 'number'); // true
console.log(typeof mem.heapTotal === 'number'); // true
console.log(typeof mem.heapUsed === 'number'); // true
console.log(mem.rss > 0); // true

console.log("All process gap tests passed!");

// Expected output:
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// true
// All process gap tests passed!
