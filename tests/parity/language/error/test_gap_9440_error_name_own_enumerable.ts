// #9440: Error-subclass construction stamped `name` as an own enumerable
// property. Node inherits the non-enumerable value from Error.prototype, so
// the wrong shape leaked through JSON, every own-key API, for-in and spread.
// Compared byte-for-byte against `node --experimental-strip-types`.

import { inspect } from "node:util";

class MyErr extends Error {}
class MyTypeErr extends TypeError {}

function ownSnapshot(label: string, error: Error): void {
    const forIn: string[] = [];
    for (const key in error) {
        forIn.push(key);
    }

    console.log(label + " value: " + error.name);
    console.log(label + " json: " + JSON.stringify(error));
    console.log(
        label + " own names: " + JSON.stringify(Object.getOwnPropertyNames(error)),
    );
    console.log(label + " keys: " + JSON.stringify(Object.keys(error)));
    console.log(label + " for-in: " + JSON.stringify(forIn));
    console.log(label + " spread: " + JSON.stringify({ ...error }));
    // Stack paths and frames are host-specific. The first line is the stable
    // Error headline that util.inspect derives from the effective name.
    console.log(label + " inspect: " + inspect(error).split("\n")[0]);
}

ownSnapshot("Error", new Error("boom"));
ownSnapshot("subclass", new MyErr("boom"));
ownSnapshot("type-subclass", new MyTypeErr("boom"));

// Assignment must still use ordinary [[Set]] semantics: it creates an own,
// writable/enumerable/configurable data property and all enumeration paths see
// exactly that one additional key.
const assigned = new MyErr("boom");
assigned.name = "Custom";
ownSnapshot("assigned", assigned);
const descriptor = Object.getOwnPropertyDescriptor(assigned, "name");
console.log(
    "assigned descriptor: " +
        JSON.stringify({
            value: descriptor?.value,
            writable: descriptor?.writable,
            enumerable: descriptor?.enumerable,
            configurable: descriptor?.configurable,
        }),
);

// The native ErrorHeader path must agree with the ordinary ObjectHeader used
// by a subclass. This also pins assignment after construction, rather than a
// class-body field initializer.
const assignedBase = new Error("base");
assignedBase.name = "CustomBase";
ownSnapshot("assigned-base", assignedBase);

// Controls for the actual prototype placement.
console.log(
    "Error.prototype.name: " +
        JSON.stringify(Object.getOwnPropertyDescriptor(Error.prototype, "name")),
);
console.log(
    "TypeError.prototype.name: " +
        JSON.stringify(Object.getOwnPropertyDescriptor(TypeError.prototype, "name")),
);
