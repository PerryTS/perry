// Program string literals are immutable images in the linker-owned
// `perry_strings` section. Every reader below validates the image header, so a
// runtime that does not own the section bounds rejects the literal itself.
import * as fs from "node:fs";
import * as path from "node:path";

console.log(path.join("alpha", "beta", "gamma.txt"));
console.log(path.basename("/tmp/dir/file.txt"), path.extname("archive.tar.gz"), path.dirname("/x/y/z"));
console.log(path.resolve("/base", "sub", "..", "leaf"));

const params = new URLSearchParams("first=1&second=two");
params.append("third", "three");
console.log(params.get("first"), params.get("second"), params.toString());

fs.writeFileSync("literal-image.txt", "literal contents");
console.log(fs.readFileSync("literal-image.txt", "utf8"), fs.existsSync("literal-image.txt"));
fs.unlinkSync("literal-image.txt");
console.log(fs.existsSync("literal-image.txt"));
