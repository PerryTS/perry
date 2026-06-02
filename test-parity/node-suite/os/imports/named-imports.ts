import { arch, platform, type, release, EOL } from "node:os";

console.log("arch known:", ["arm", "arm64", "ia32", "loong64", "mips", "mipsel", "ppc", "ppc64", "riscv64", "s390", "s390x", "x64"].includes(arch()));
console.log("platform string:", typeof platform() === "string");
console.log("type release strings:", typeof type() === "string" && typeof release() === "string");
console.log("EOL string:", typeof EOL === "string");
