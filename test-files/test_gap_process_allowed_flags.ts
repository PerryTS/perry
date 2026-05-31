// Gap test: process.allowedNodeEnvironmentFlags (#2589).
//
// Node exposes this as a special read-only Set of flags accepted from
// NODE_OPTIONS. The exact membership varies by Node build, so this test
// only asserts the stable, structural contract: it is a non-empty Set
// with the usual Set methods, `instanceof Set` holds, a few stable flags
// are present, an unknown flag is absent, and iteration count matches
// `.size`. These checks are byte-identical against Node.

const flags = process.allowedNodeEnvironmentFlags;

console.log(flags instanceof Set);
console.log(flags.size > 0);
console.log(typeof flags.has === "function");
console.log(typeof flags.add === "function");
console.log(typeof flags.delete === "function");
console.log(typeof flags.forEach === "function");

// A few flags that are stable across modern Node builds.
console.log(flags.has("--no-warnings"));
console.log(flags.has("--max-http-header-size"));
console.log(flags.has("--inspect"));
console.log(flags.has("-r"));

// An unknown flag is not a member.
console.log(flags.has("--this-flag-does-not-exist"));

// Iteration visits exactly `.size` entries.
let count = 0;
flags.forEach(() => {
  count++;
});
console.log(count === flags.size);
