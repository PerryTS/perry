// #9411 — `#x in o` (ES2022 ergonomic brand check, ECMA-262 §13.10.2) answered
// `false` for objects that really do carry the private field. Downleveled
// TypeScript/Babel output uses exactly this form to ask "is this one of mine?",
// so the wrong answer is a silently-taken wrong branch, not an error.
//
// The pre-existing fixtures only covered the brand check from an *instance*
// method; the reported failure is the check evaluated from a *static* method
// (and from a static block / static field initializer), which is the shape
// `class A { #x = 1; static has(o) { return #x in o } }` — the one downlevelers
// emit for `WeakMap`-free brand checks.

function log(label: string, value: unknown): void {
    console.log(label + ": " + String(value));
}

class FieldBrand {
    #x = 1;

    static has(o: any): boolean {
        return #x in o;
    }

    static hasArrow: (o: any) => boolean = (o: any) => #x in o;

    static fromBlock = false;

    static {
        FieldBrand.fromBlock = #x in new FieldBrand();
    }

    hasFromInstance(o: any): boolean {
        return #x in o;
    }

    read(): number {
        return this.#x;
    }
}

class MethodBrand {
    #m(): string {
        return "m";
    }

    static has(o: any): boolean {
        return #m in o;
    }

    call(): string {
        return this.#m();
    }
}

class AccessorBrand {
    get #g(): string {
        return "g";
    }
    set #s(_value: string) {}

    static hasGetter(o: any): boolean {
        return #g in o;
    }

    static hasSetter(o: any): boolean {
        return #s in o;
    }
}

class StaticFieldBrand {
    static #s = 1;

    static has(o: any): boolean {
        return #s in o;
    }
}

class FieldSubclass extends FieldBrand {}
class MethodSubclass extends MethodBrand {}

class Foreign {
    #x = 1;
}

const instance = new FieldBrand();
const subclassInstance = new FieldSubclass();

log("field brand from static method", FieldBrand.has(instance));
log("field brand from static arrow", FieldBrand.hasArrow(instance));
log("field brand from static block", FieldBrand.fromBlock);
log("field brand from instance method", instance.hasFromInstance(instance));
log("field brand on subclass instance (static)", FieldBrand.has(subclassInstance));
log(
    "field brand on subclass instance (instance)",
    instance.hasFromInstance(subclassInstance)
);
log("field read still works", instance.read());

log("method brand from static method", MethodBrand.has(new MethodBrand()));
log("method brand on subclass instance", MethodBrand.has(new MethodSubclass()));
log("method call still works", new MethodBrand().call());

log("getter brand from static method", AccessorBrand.hasGetter(new AccessorBrand()));
log("setter brand from static method", AccessorBrand.hasSetter(new AccessorBrand()));

log("static field brand on constructor", StaticFieldBrand.has(StaticFieldBrand));

// Negatives — every one of these must stay false.
log("field brand on plain object", FieldBrand.has({}));
log("field brand on public hash key", FieldBrand.has({ "#x": 1 }));
log("field brand on foreign class instance", FieldBrand.has(new Foreign()));
log("field brand on constructor itself", FieldBrand.has(FieldBrand));
log("field brand on prototype", FieldBrand.has(FieldBrand.prototype));
log("method brand on plain object", MethodBrand.has({}));
log("method brand on foreign instance", MethodBrand.has(new Foreign()));
log("static field brand on instance", StaticFieldBrand.has(new FieldBrand()));
log("field brand on array", FieldBrand.has([]));
log("field brand on function", FieldBrand.has(function () {}));

// A superclass brand is visible on a subclass instance, but a subclass brand is
// NOT visible on a bare superclass instance.
class Base {
    #b = 1;
    static hasBase(o: any): boolean {
        return #b in o;
    }
}
class Derived extends Base {
    #d = 2;
    static hasDerived(o: any): boolean {
        return #d in o;
    }
}
log("base brand on derived instance", Base.hasBase(new Derived()));
log("derived brand on derived instance", Derived.hasDerived(new Derived()));
log("derived brand on base instance", Derived.hasDerived(new Base()));

// Two separate evaluations of the same class body produce distinct brands.
function makeClass(): any {
    return class {
        #k = 1;
        static has(o: any): boolean {
            return #k in o;
        }
    };
}
const First = makeClass();
const Second = makeClass();
log("fresh brand own instance", First.has(new First()));
log("fresh brand cross evaluation", First.has(new Second()));
