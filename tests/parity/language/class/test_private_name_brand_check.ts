let failures = 0;

function check(label: string, ok: boolean): void {
    if (!ok) {
        failures = failures + 1;
        console.log("FAIL " + label);
    }
}

class BrandOwner {
    #x = 1;

    ownsSelf(): boolean {
        return #x in this;
    }

    owns(value: any): boolean {
        return #x in value;
    }
}

class OtherBrandOwner {
    #x = 1;
}

const owner = new BrandOwner();
check("#x in this", owner.ownsSelf() === true);
check("#x false for plain object", owner.owns({}) === false);
check("#x false for public hash key", owner.owns({ "#x": 1 }) === false);
check("#x false for unrelated class", owner.owns(new OtherBrandOwner()) === false);

class UndiciShapedIterator {
    #target: string[];
    #kind: string;
    #index = 0;

    constructor(target: string[], kind: string) {
        this.#target = target;
        this.#kind = kind;
    }

    next(): any {
        if (!(#target in this)) {
            return { done: true, value: undefined };
        }
        if (this.#index >= this.#target.length) {
            return { done: true, value: undefined };
        }
        const index = this.#index;
        this.#index = this.#index + 1;
        if (this.#kind === "key") {
            return { done: false, value: index };
        }
        return { done: false, value: this.#target[index] };
    }
}

const iterator = new UndiciShapedIterator(["a", "b"], "value");
const first = iterator.next();
const second = iterator.next();
const third = iterator.next();
check("undici iterator first value", first.done === false && first.value === "a");
check("undici iterator second value", second.done === false && second.value === "b");
check("undici iterator done", third.done === true);

const detachedNext = iterator.next;
const detachedResult = detachedNext.call({});
check("undici iterator guard false for non-instance", detachedResult.done === true);

function createIterator(name: string, internalIterator: (target: string[]) => string[]) {
    return class FastIterableIterator {
        #target: string[];
        #kind: "key" | "value" | "key+value";
        #index: number;

        constructor(target: string[], kind: "key" | "value" | "key+value") {
            this.#target = target;
            this.#kind = kind;
            this.#index = 0;
        }

        next(): any {
            if (typeof this !== "object" || this === null || !(#target in this)) {
                throw new TypeError(
                    "'next' called on an object that does not implement interface " +
                        name +
                        " Iterator."
                );
            }

            const values = internalIterator(this.#target);
            if (this.#index >= values.length) return { value: undefined, done: true };
            const index = this.#index;
            this.#index = this.#index + 1;
            if (this.#kind === "key") return { value: index, done: false };
            if (this.#kind === "value") return { value: values[index], done: false };
            return { value: [index, values[index]], done: false };
        }
    };
}

const IteratorClass = createIterator("Headers", (target) => target);
const factoryIterator = new IteratorClass(["content-type"], "value");
const factoryNext = factoryIterator.next();
check(
    "undici factory-returned iterator brand guard",
    factoryNext.done === false && factoryNext.value === "content-type"
);

if (failures !== 0) {
    throw new Error("private brand check failures: " + failures.toString());
}

console.log("private name brand checks ok");
