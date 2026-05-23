// POSIX credential accessors (#1408). On the macOS / Linux runners these
// all return numeric ids; the test just probes the shape so it's
// reproducible across users.
console.log("uid is number:", typeof process.getuid() === "number");
console.log("euid is number:", typeof process.geteuid() === "number");
console.log("gid is number:", typeof process.getgid() === "number");
console.log("egid is number:", typeof process.getegid() === "number");
