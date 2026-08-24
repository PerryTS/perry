function check(label: string, condition: boolean): void {
    console.log(label + ": " + condition.toString());
}

function throwsTypeError(callback: () => void): boolean {
    try {
        callback();
        return false;
    } catch (error) {
        return error instanceof TypeError;
    }
}

function makeDeclarationInstance(): any {
    class C {
        #value = "test262";

        #method(): string {
            return this.#value;
        }

        get #getter(): string {
            return this.#value;
        }

        set #setter(value: string) {
            this.#value = value;
        }

        readMethod(other: C): string {
            return other.#method();
        }

        readGetter(other: C): string {
            return other.#getter;
        }

        writeSetter(other: C, value: string): void {
            other.#setter = value;
        }

        hasValue(other: any): boolean {
            return #value in other;
        }
    }

    return new C();
}

function makeExpressionInstance(): any {
    const C = class {
        #value = "test262";

        #method(): string {
            return this.#value;
        }

        get #getter(): string {
            return this.#value;
        }

        set #setter(value: string) {
            this.#value = value;
        }

        readMethod(other: any): string {
            return other.#method();
        }

        readGetter(other: any): string {
            return other.#getter;
        }

        writeSetter(other: any, value: string): void {
            other.#setter = value;
        }

        hasValue(other: any): boolean {
            return #value in other;
        }
    };

    return new C();
}

function checkFreshBrands(label: string, make: () => any): void {
    const first = make();
    const second = make();

    check(label + " own method", first.readMethod(first) === "test262");
    check(label + " own getter", second.readGetter(second) === "test262");
    first.writeSetter(first, "changed");
    check(label + " own setter", first.readGetter(first) === "changed");
    check(
        label + " setter isolation",
        second.readGetter(second) === "test262"
    );
    check(label + " own in", first.hasValue(first));
    check(label + " cross-evaluation in", !first.hasValue(second));

    check(
        label + " cross-evaluation method",
        throwsTypeError(() => first.readMethod(second))
    );
    check(
        label + " cross-evaluation getter",
        throwsTypeError(() => first.readGetter(second))
    );
    check(
        label + " cross-evaluation setter",
        throwsTypeError(() => first.writeSetter(second, "wrong"))
    );
    check(
        label + " failed setter isolation",
        second.readGetter(second) === "test262"
    );
    check(
        label + " own state after throw",
        first.readGetter(first) === "changed"
    );
}

checkFreshBrands("declaration", makeDeclarationInstance);
checkFreshBrands("expression", makeExpressionInstance);

function makeStaticClass(): any {
    return class {
        static #value = "test262";
        static _written = "";

        static #method(): string {
            return this.#value;
        }

        static get #getter(): string {
            return this.#value;
        }

        static set #setter(value: string) {
            this._written = value;
        }

        static accessMethod(): string {
            return this.#method();
        }

        static accessGetter(): string {
            return this.#getter;
        }

        static accessSetter(value: string): void {
            this.#setter = value;
        }

        static hasValue(other: any): boolean {
            return #value in other;
        }
    };
}

function makeStaticDeclarationClass(): any {
    class StaticDeclarationC {
        static #value = "test262";
        static _written = "";

        static #method(): string {
            return this.#value;
        }

        static get #getter(): string {
            return this.#value;
        }

        static set #setter(value: string) {
            this._written = value;
        }

        static accessMethod(): string {
            return this.#method();
        }

        static accessGetter(): string {
            return this.#getter;
        }

        static accessSetter(value: string): void {
            this.#setter = value;
        }

        static hasValue(other: any): boolean {
            return #value in other;
        }
    }

    return StaticDeclarationC;
}

function checkFreshStaticBrands(label: string, make: () => any): void {
    const first = make();
    const second = make();
    check(label + " own method", first.accessMethod() === "test262");
    check(label + " own getter", second.accessGetter() === "test262");
    first.accessSetter("changed");
    check(label + " own setter", first._written === "changed");
    check(label + " setter isolation", second._written === "");
    check(label + " own in", first.hasValue(first));
    check(label + " cross-evaluation in", !first.hasValue(second));
    check(
        label + " cross-evaluation method",
        throwsTypeError(() => first.accessMethod.call(second))
    );
    check(
        label + " cross-evaluation getter",
        throwsTypeError(() => first.accessGetter.call(second))
    );
    check(
        label + " own getter after throw",
        first.accessGetter() === "test262"
    );
    check(
        label + " cross-evaluation setter",
        throwsTypeError(() => first.accessSetter.call(second, "wrong"))
    );
    check(label + " failed setter isolation", second._written === "");
    check(label + " own state after throw", first._written === "changed");
}

checkFreshStaticBrands("static expression", makeStaticClass);
checkFreshStaticBrands("static declaration", makeStaticDeclarationClass);

function makeOrderedStatics(label: string): any {
    const events: string[] = [];
    const key = (name: string): string => {
        events.push("key-" + name);
        return name;
    };
    const C = class {
        static #brand = 0;

        [key("method")](): void {}

        static [key("first")] = (events.push("init-first"), 1);

        static {
            events.push("block");
            (this as any).fromBlock = label;
        }

        static tail = (events.push("init-tail"), 2);
        static missing;
    };

    check(
        label + " computed/static order",
        events.join(",") ===
            "key-method,key-first,init-first,block,init-tail"
    );
    check(label + " static block this", C.fromBlock === label);
    check(
        label + " uninitialized static own",
        Object.prototype.hasOwnProperty.call(C, "missing")
    );
    check(label + " uninitialized static value", C.missing === undefined);
    return C;
}

makeOrderedStatics("fresh order");

function makeOrderedDeclarationStatics(label: string): any {
    const events: string[] = [];
    const key = (name: string): string => {
        events.push("key-" + name);
        return name;
    };
    class C {
        static #brand = 0;

        [key("method")](): void {}

        static [key("first")] = (events.push("init-first"), 1);

        static {
            events.push("block");
            (this as any).fromBlock = label;
        }

        static tail = (events.push("init-tail"), 2);
        static missing;
    }

    check(
        label + " computed/static order",
        events.join(",") ===
            "key-method,key-first,init-first,block,init-tail"
    );
    check(label + " static block this", C.fromBlock === label);
    check(
        label + " uninitialized static own",
        Object.prototype.hasOwnProperty.call(C, "missing")
    );
    check(label + " uninitialized static value", C.missing === undefined);
    return C;
}

makeOrderedDeclarationStatics("fresh declaration order");

const sharedTemplateEvents: string[] = [];
const SharedTemplateOrder = class {
    static first = (sharedTemplateEvents.push("first"), 1);

    static {
        sharedTemplateEvents.push("block");
    }

    static last = (sharedTemplateEvents.push("last"), 2);
};
check(
    "shared-template static order",
    sharedTemplateEvents.join(",") === "first,block,last" &&
        SharedTemplateOrder.last === 2
);

function makeStaticBlockOnlyClass(): any {
    return class {
        static {
            (this as any).n = ((this as any).n ?? 0) + 1;
        }
    };
}

const staticBlockOnlyFirst = makeStaticBlockOnlyClass();
const staticBlockOnlySecond = makeStaticBlockOnlyClass();
check(
    "static-block-only class identity",
    staticBlockOnlyFirst !== staticBlockOnlySecond
);
check(
    "static-block-only class state",
    staticBlockOnlyFirst.n === 1 && staticBlockOnlySecond.n === 1
);

function makeMutableParameter(value: string): any {
    class MutableParameter {
        read(): string {
            return value;
        }

        write(next: string): void {
            value = next;
        }
    }

    return new MutableParameter();
}

function makeUnrelatedParameter(value: string): any {
    class UnrelatedParameter {
        read(): string {
            return value;
        }
    }

    return new UnrelatedParameter();
}

const mutableParameter = makeMutableParameter("mutable");
const unrelatedParameter = makeUnrelatedParameter("unrelated");
mutableParameter.write("changed");
check("shared parameter mutation", mutableParameter.read() === "changed");
check(
    "shared parameter owner isolation",
    unrelatedParameter.read() === "unrelated"
);

function makeDynamicAccessor(tag: string): any {
    return class {
        static #brand = 0;

        static install(): void {
            Object.defineProperty(this, "dynamic", {
                configurable: true,
                enumerable: true,
                get: () => tag,
                set: (value: string) => {
                    tag = value;
                },
            });
        }

        static readTag(): string {
            return tag;
        }
    };
}

const accessorA = makeDynamicAccessor("a");
const accessorB = makeDynamicAccessor("b");
accessorA.install();
accessorB.install();
check("dynamic accessor evaluation A", accessorA.dynamic === "a");
check("dynamic accessor evaluation B", accessorB.dynamic === "b");
Object.defineProperty(accessorA, "dynamic", {
    get: () => "fixed",
});
const accessorDescriptor = Object.getOwnPropertyDescriptor(
    accessorA,
    "dynamic"
)!;
check(
    "dynamic accessor retained halves",
    typeof accessorDescriptor.set === "function"
);
check(
    "dynamic accessor retained attrs",
    accessorDescriptor.enumerable && accessorDescriptor.configurable
);
accessorA.dynamic = "changed";
check("dynamic accessor retained setter", accessorA.readTag() === "changed");
check("dynamic accessor sibling isolated", accessorB.dynamic === "b");

function NullPrototypeParent(): void {}
NullPrototypeParent.prototype = null;
function makeNullPrototypeChild(): any {
    return class extends NullPrototypeParent {
        static #brand = 0;
    };
}
const nullPrototypeChild = makeNullPrototypeChild();
check(
    "fresh null parent prototype",
    Object.getPrototypeOf(nullPrototypeChild.prototype) === null
);
check(
    "fresh class Function constructor",
    nullPrototypeChild.constructor === Function
);

const internalPrefixObject: any = {};
internalPrefixObject["#<perry:user>"] = 1;
check(
    "user perry-prefix key enumerable",
    Object.keys(internalPrefixObject).includes("#<perry:user>")
);

const lazyJson = "[" + new Array(600).fill("0").join(",") + "]";
function ReturnLazyArray(): any {
    return JSON.parse(lazyJson);
}
const lazyConstructorResult = new (ReturnLazyArray as any)();
check(
    "lazy array constructor return override",
    Array.isArray(lazyConstructorResult) && lazyConstructorResult.length === 600
);

function makePrototypeParent(tag: string): any {
    return class {
        static #brand = 0;

        inherited(): string {
            return tag;
        }
    };
}

function makePrototypeChild(parent: any): any {
    return class extends parent {
        static #brand = 0;
    };
}

const prototypeParent = makePrototypeParent("parent");
const prototypeChild = makePrototypeChild(prototypeParent);
check(
    "fresh prototype parent link",
    Object.getPrototypeOf(prototypeChild.prototype) === prototypeParent.prototype
);
check(
    "fresh prototype inherited method",
    new prototypeChild().inherited() === "parent"
);

class HugeKeyBase {}
Object.defineProperty(HugeKeyBase.prototype, "9223372036854776000", {
    value: "huge",
});
class HugeKeyDerived extends HugeKeyBase {
    read(): string {
        return super[9223372036854775808];
    }
}
check("super huge numeric property key", new HugeKeyDerived().read() === "huge");

const boxedString = new String("payload");
check("boxed String toString payload", boxedString.toString() === "payload");
check("boxed String valueOf payload", boxedString.valueOf() === "payload");
