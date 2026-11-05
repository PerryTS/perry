// Refs #2135: `process.chdir()` to a missing / non-directory target previously
// no-op'd silently. Node throws an `Error` with `code` / `syscall` / `path`
// set and a libuv-formatted message:
//   ENOENT: no such file or directory, chdir '<cwd>' -> '<target>'
// The runtime now mirrors that shape so user code that catches and inspects
// the error matches Node byte-for-byte.

const origCwd = process.cwd();

// Success round-trip: chdir-then-chdir-back leaves the cwd unchanged.
process.chdir("/tmp");
console.log(process.cwd());
process.chdir(origCwd);
console.log(process.cwd() === origCwd);

// Missing directory → ENOENT
try {
    process.chdir("/this/dir/does/not/exist_perry_2135");
    console.log("no throw");
} catch (e: any) {
    console.log(e.code, e.syscall);
}

// Existing path that is not a directory → ENOTDIR (every Unix host has /etc/hosts)
try {
    process.chdir("/etc/hosts");
    console.log("no throw");
} catch (e: any) {
    console.log(e.code, e.syscall);
}
