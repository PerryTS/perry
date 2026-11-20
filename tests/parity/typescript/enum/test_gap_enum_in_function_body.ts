// An enum declared inside a FUNCTION BODY is valid TypeScript. Perry only
// registered enums at module scope, so `lower_body_stmt` bailed with
// "enum declared inside a function body is not supported". Registration in the
// lowering context is what makes the name resolve; the declaration must also
// reach `Module::enums`, which is what codegen consults to resolve
// `Expr::EnumMember`.
//
// Found via the Vercel CLI corpus
// (packages/build-utils/src/ruby-diagnostics.ts declares `Section` and
// `SubSection` inside `parseGemfileLock`).

// String enum local to a function, driving assignment + comparison — the
// ruby-diagnostics shape.
function parse(content: string): string[] {
    enum Section {
        GEM = "gem",
        GIT = "git",
        DEPENDENCIES = "dependencies",
    }

    let section: Section | null = null;
    const out: string[] = [];

    for (const line of content.split("\n")) {
        if (line === "GEM") section = Section.GEM;
        else if (line === "GIT") section = Section.GIT;
        else if (line === "DEPENDENCIES") section = Section.DEPENDENCIES;
        if (section === Section.GEM) out.push("gem:" + line);
    }
    return out;
}
console.log("parse:", parse("GEM\nfoo\nGIT\nbar").join(",")); // gem:GEM,gem:foo

// Numeric (auto-incremented) enum in a function body, including the reverse
// mapping a numeric enum carries.
function levels(): string {
    enum Level {
        LOW,
        MEDIUM,
        HIGH,
    }
    return `${Level.LOW},${Level.MEDIUM},${Level.HIGH},${Level[2]}`;
}
console.log("levels:", levels()); // 0,1,2,HIGH

// Explicit numeric values, read through a switch.
function classify(n: number): string {
    enum Code {
        OK = 200,
        NOT_FOUND = 404,
    }
    switch (n) {
        case Code.OK:
            return "ok";
        case Code.NOT_FOUND:
            return "missing";
        default:
            return "other";
    }
}
console.log("classify:", classify(404), classify(200), classify(1)); // missing ok other

// A body-local enum inside a nested function.
function outer(): string {
    function inner(): string {
        enum Inner {
            A = "a",
            B = "b",
        }
        return Inner.A + Inner.B;
    }
    return inner();
}
console.log("nested:", outer()); // ab

// A body-local enum must not disturb a module-scope enum of a different name.
enum Global {
    ONE = "one",
}
function usesGlobal(): string {
    enum Local {
        TWO = "two",
    }
    return Global.ONE + "/" + Local.TWO;
}
console.log("mixed:", usesGlobal()); // one/two
