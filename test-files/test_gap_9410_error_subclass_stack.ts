// #9410 — `class X extends Error {}` produced instances with no `.stack` and a
// `[object Object]` tag. Only subclasses were affected; the base `Error` was
// fine, which is why no fixture caught it. The cc bundle has 93 `extends Error`
// classes, so every diagnostic it printed lost its trace.
//
// Stack CONTENTS are host-specific (absolute paths, frame counts), so this
// fixture asserts only what is portable: that `.stack` is a string that starts
// with the constructor name and the message, that `Object.prototype.toString`
// reports `[object Error]`, and that the ordinary Error surface
// (`name`/`message`/`instanceof`) is intact.

class Plain extends Error {}

class Named extends Error {
    constructor(message: string) {
        super(message);
        this.name = "Named";
    }
}

class WithField extends Error {
    code = "E_FIELD";
}

class Deep extends Named {}

class Late extends Error {
    constructor(message: string) {
        super();
        this.message = message;
    }
}

class TypeErrorSub extends TypeError {}
class RangeErrorSub extends RangeError {}

function describe(label: string, error: any, expectedName: string, expectedMessage: string): void {
    const stack = error.stack;
    console.log(label + " stack typeof: " + typeof stack);
    console.log(label + " stack nonempty: " + (typeof stack === "string" && stack.length > 0));
    console.log(
        label + " stack head: " +
            (typeof stack === "string"
                ? stack.split("\n")[0]
                : "<missing>")
    );
    console.log(label + " tag: " + Object.prototype.toString.call(error));
    console.log(label + " name: " + error.name);
    console.log(label + " message: " + error.message);
    console.log(label + " instanceof Error: " + (error instanceof Error));
    console.log(label + " toString: " + String(error));
    console.log(
        label + " own stack or inherited: " +
            (typeof stack === "string" || stack === undefined ? "reported" : "other")
    );
    console.log(label + " name matches: " + (error.name === expectedName));
    console.log(label + " message matches: " + (error.message === expectedMessage));
}

describe("base", new Error("base-msg"), "Error", "base-msg");
describe("plain-subclass", new Plain("plain-msg"), "Error", "plain-msg");
describe("named-subclass", new Named("named-msg"), "Named", "named-msg");
describe("field-subclass", new WithField("field-msg"), "Error", "field-msg");
describe("deep-subclass", new Deep("deep-msg"), "Named", "deep-msg");
describe("late-message", new Late("late-msg"), "Error", "late-msg");
describe("typeerror-subclass", new TypeErrorSub("te-msg"), "TypeError", "te-msg");
describe("rangeerror-subclass", new RangeErrorSub("re-msg"), "RangeError", "re-msg");

// A subclass instance created through a factory (the shape cc's bundle uses).
function make(message: string): Plain {
    return new Plain(message);
}
describe("factory-subclass", make("factory-msg"), "Error", "factory-msg");

// The extra field survives, and the subclass's own property is enumerable while
// `stack` is not (node installs `stack` as a non-enumerable own property).
const withField = new WithField("field-msg");
console.log("field value: " + withField.code);
// The subclass's own field enumerates; `stack` must not. NOT asserted here:
// the full `Object.keys` list, because perry additionally stamps an own
// ENUMERABLE `name` onto an Error-subclass instance where node leaves `name`
// on `Error.prototype` — a separate, pre-existing divergence (perry
// `["code","name"]` vs node `["code"]`) with its own fix, and asserting the
// whole list here would tie this fixture to that one.
console.log("field key enumerates: " + Object.keys(withField).includes("code"));
console.log("stack key enumerates: " + Object.keys(withField).includes("stack"));
console.log(
    "stack own: " + Object.prototype.hasOwnProperty.call(withField, "stack")
);
const stackDescriptor = Object.getOwnPropertyDescriptor(withField, "stack");
console.log(
    "stack enumerable: " +
        (stackDescriptor === undefined ? "<no own stack>" : String(stackDescriptor.enumerable))
);

// A caught subclass error keeps its stack through the throw.
try {
    throw new Plain("thrown-msg");
} catch (error: any) {
    console.log("caught stack typeof: " + typeof error.stack);
    console.log("caught tag: " + Object.prototype.toString.call(error));
}

// `Error.captureStackTrace`, when present, must also work on a subclass.
console.log(
    "captureStackTrace present: " +
        (typeof (Error as any).captureStackTrace === "function")
);
if (typeof (Error as any).captureStackTrace === "function") {
    const target = new Plain("capture-msg");
    (Error as any).captureStackTrace(target);
    console.log("captured stack typeof: " + typeof target.stack);
}

// Non-Error classes must NOT gain the Error tag.
class NotAnError {}
console.log("non-error tag: " + Object.prototype.toString.call(new NotAnError()));
console.log("plain-object tag: " + Object.prototype.toString.call({}));
