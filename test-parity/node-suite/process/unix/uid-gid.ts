// POSIX credential accessors are functions returning numeric ids.
console.log("getuid:", typeof process.getuid === "function");
console.log("geteuid:", typeof process.geteuid === "function");
console.log("getgid:", typeof process.getgid === "function");
console.log("getegid:", typeof process.getegid === "function");
