import * as fs from "node:fs";
import { readFileSync, writeFileSync, unlinkSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
const path = join(tmpdir(), `perry-nmi-${process.pid}`);
writeFileSync(path, "export value\n");
try {
    const f = readFileSync;
    console.log(f === fs.readFileSync, f.name, f.length);
    console.log(f(path, "utf8").trim());
    console.log([readFileSync].map(read => read(path, "utf8").trim()).join("|"));
    console.log(f.call(undefined, path, "utf8").trim());
    console.log(f.apply(undefined, [path, "utf8"]).trim());
    const bound = f.bind(undefined, path);
    console.log(bound.length, bound("utf8").trim());
    const j = join;
    console.log(["a", "b", "c"].map(p => j("prefix", p)).join("|"));
    console.log(j(...["a", "b", "c", "d", "e", "f", "g", "h"]));
} finally {
    unlinkSync(path);
}
