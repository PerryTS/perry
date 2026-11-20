// #9440 — an Error subclass inherited `.name` from the Error-family
// prototype in Node, but Perry stamped it onto every instance as an enumerable
// own property. That leaked `name` through every own-key consumer.
//
// Keep all output portable and byte-comparable with
// `node --experimental-strip-types`: stack frames contain host paths, so the
// util.inspect check removes only `at ...` lines while retaining the Error
// headline and any rendered properties.

import { inspect } from "node:util";

class Implicit extends Error {}

class Explicit extends Error {
    constructor(message: string) {
        super(message);
    }
}

class Deep extends Implicit {
    constructor(message: string) {
        super(message);
    }
}

class TypeSubclass extends TypeError {}

function forInKeys(value: object): string[] {
    const keys: string[] = [];
    for (const key in value) {
        keys.push(key);
    }
    return keys;
}

function stableInspect(value: unknown): string {
    return inspect(value, { breakLength: Infinity }).replace(
        /\n\s+at [^\n]*/g,
        "",
    );
}

function describe(label: string, error: Error): void {
    console.log(
        label +
            " reflection: " +
            JSON.stringify({
                name: error.name,
                ownName: Object.prototype.hasOwnProperty.call(error, "name"),
                descriptor: Object.getOwnPropertyDescriptor(error, "name"),
                json: JSON.stringify(error),
                ownNames: Object.getOwnPropertyNames(error),
                keys: Object.keys(error),
                forIn: forInKeys(error),
                spread: { ...error },
            }),
    );
    console.log(label + " inspect: " + JSON.stringify(stableInspect(error)));
}

describe("base", new Error("base"));
describe("base-empty", new Error());
describe("implicit", new Implicit("implicit"));
describe("implicit-empty", new Implicit());
describe("explicit", new Explicit("explicit"));
describe("deep", new Deep("deep"));
describe("type-subclass", new TypeSubclass("typed"));

// Exercise dynamic construction, which runs the synthesized standalone
// constructor rather than the direct-new initialization path.
const Dynamic: typeof Implicit = Implicit;
describe("dynamic", new Dynamic("dynamic"));

function makeEscapedSubclass(): typeof Error {
    return class Escaped extends Error {};
}

// Force the runtime constructor-replay path: the concrete subclass is created
// inside a function and only reaches this construction site as a value.
const Escaped = makeEscapedSubclass();
describe("escaped-dynamic", new Escaped("escaped-dynamic"));

// An explicit assignment must still create an ordinary own enumerable
// property, just as it does for any inherited writable data property.
const custom = new Implicit("custom");
custom.name = "Custom";
describe("assigned", custom);

const customBase = new Error("custom-base");
customBase.name = "CustomBase";
describe("assigned-base", customBase);
