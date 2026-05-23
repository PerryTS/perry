// process.platform / process.arch report the host (same machine for Node and
// Perry, so the values match) and are drawn from the known enums.
const platforms = ["aix", "darwin", "freebsd", "linux", "openbsd", "sunos", "win32"];
const arches = ["arm", "arm64", "ia32", "loong64", "mips", "mipsel", "ppc64", "riscv64", "s390x", "x64"];
console.log("platform:", process.platform);
console.log("arch:", process.arch);
console.log("platform known:", platforms.includes(process.platform));
console.log("arch known:", arches.includes(process.arch));
