// Regression test for closure capture of module-level Map after v0.4.53
// filter_module_level_captures change.
//
// Bug: closures that call methods on module-level Maps/Sets/objects silently
// returned garbage because the new filter_module_level_captures stripped the
// Map's LocalId from captures, but the codegen auto-loader's walker
// (collect_referenced_locals_stmts) didn't cover all Expr variants, so
// references to the Map inside the closure resolved to "undefined".
//
// Production symptom: perry-hub's /api/v1/build route dispatched jobs with
// manifest=null because the async handler's try-block references to the
// parsed `manifest` object were looked up against an empty locals map.

const accounts = new Map<string, string>();
accounts.set("alice", "admin");
accounts.set("bob", "user");

const allowedRoles = new Set<string>();
allowedRoles.add("admin");
allowedRoles.add("user");

const users: string[] = ["alice", "bob", "charlie"];

// Closure capturing module-level Map + Set
users.forEach((name: string) => {
    const role = accounts.get(name) || "unknown";
    const allowed = allowedRoles.has(role);
    console.log(name + " -> " + role + " allowed=" + String(allowed));
});

// Closure in a function that uses Map methods
function countKnown(): number {
    let count = 0;
    users.forEach((name: string) => {
        if (accounts.has(name)) {
            count++;
        }
    });
    return count;
}
console.log("known: " + String(countKnown()));

// Nested closures both referencing module-level vars
function buildReport(): string[] {
    const report: string[] = [];
    users.forEach((name: string) => {
        const role = accounts.get(name) || "none";
        report.push(name + ":" + role);
    });
    return report;
}
console.log("report: " + buildReport().join(","));
