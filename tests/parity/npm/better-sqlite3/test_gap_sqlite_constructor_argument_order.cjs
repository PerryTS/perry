const Better = require("better-sqlite3");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "perry-sqlite-args-"));
function filename(leaf) {
    console.log("path:" + leaf);
    return path.join(dir, leaf);
}
function options() {
    console.log("options");
    return {};
}
function trailing(label) {
    console.log(label);
    return { label };
}
function fail() {
    console.log("throw");
    throw new Error("sentinel");
}
try {
    const db = new Better(filename("ok.db"), options(), trailing("third"), trailing("fourth"));
    console.log("row=" + db.prepare("SELECT 22 AS n").get().n);
    db.close();
    try {
        const wrong = new Better(filename("absent.db"), options(), fail(), trailing("unreachable"));
        wrong.close();
        console.log("unexpected open");
    } catch (e) {
        console.log("caught=" + e.message);
    }
    console.log("ok-exists=" + fs.existsSync(path.join(dir, "ok.db")));
    console.log("absent-exists=" + fs.existsSync(path.join(dir, "absent.db")));
} finally {
    fs.rmSync(dir, { recursive: true, force: true });
}
