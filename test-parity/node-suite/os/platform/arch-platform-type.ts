import * as os from "node:os";

console.log("arch known:", ["arm", "arm64", "ia32", "loong64", "mips", "mipsel", "ppc", "ppc64", "riscv64", "s390", "s390x", "x64"].includes(os.arch()));
console.log("platform known:", ["aix", "darwin", "freebsd", "linux", "openbsd", "sunos", "win32"].includes(os.platform()));
console.log("type nonempty:", typeof os.type() === "string" && os.type().length > 0);
