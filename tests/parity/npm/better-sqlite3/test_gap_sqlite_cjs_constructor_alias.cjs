const Better = require("better-sqlite3");
const db = new Better(":memory:");
console.log(db.prepare("SELECT 22 AS n").get().n);
db.close();

class Local {
    constructor(value) { this.value = value; }
}
function construct(Better) {
    return new Better("local").value;
}
console.log(construct(Local));
