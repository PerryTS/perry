class Counter {
    #count = 0;

    explicit(): number {
        this.#count = this.#count + 1;
        return this.#count;
    }

    readInExpr(): number {
        const value = this.#count;
        return value + 1;
    }

    compound(): number {
        this.#count += 1;
        return this.#count;
    }

    logical(): number {
        this.#count ||= 10;
        this.#count &&= 4;
        this.#count ??= 20;
        return this.#count;
    }

    postfix(): number {
        return this.#count++;
    }

    prefix(): number {
        return ++this.#count;
    }

    read(): number {
        return this.#count;
    }
}

const compound = new Counter();
console.log("explicit", compound.explicit());
console.log("readInExpr", compound.readInExpr());
console.log("compound", compound.compound());
console.log("logical", compound.logical());

// Exercise update lowering on a fresh instance so a failed compound write
// cannot poison the observation.
const update = new Counter();
console.log("postfix-result", update.postfix());
console.log("postfix-value", update.read());
console.log("prefix-result", update.prefix());
console.log("prefix-value", update.read());

class Hidden {
    #value = 5;

    read(): number {
        return this.#value;
    }
}

const hidden = new Hidden();
console.log("hidden-read", hidden.read());
console.log("own-names", JSON.stringify(Object.getOwnPropertyNames(hidden)));
console.log("keys", JSON.stringify(Object.keys(hidden)));
console.log("json", JSON.stringify(hidden));
console.log("spread", JSON.stringify({ ...hidden }));
let forIn = "";
for (const key in hidden) {
    forIn += key;
}
console.log("for-in", forIn);

// A compiler routing spelling used as ordinary user data must remain an
// ordinary property when no private-access hint accompanies it.
const collisionKey = "#<perry:private-member:1:x>";
const collision: Record<string, number> = {};
collision[collisionKey] = 8;
console.log("collision-json", JSON.stringify(collision));
console.log("collision-keys", JSON.stringify(Object.keys(collision)));
console.log("collision-names", JSON.stringify(Object.getOwnPropertyNames(collision)));
console.log("collision-read", collision[collisionKey]);
console.log("collision-in", collisionKey in collision);
console.log("collision-own", Object.hasOwn(collision, collisionKey));

// Private fields must not consume or shift public shape slots, including
// across an inheritance chain.
class Parent {
    parent = 1;
    #parentSecret = 2;

    parentTotal(): number {
        return this.parent + this.#parentSecret;
    }
}

class Child extends Parent {
    child = 3;
    #childSecret = 4;

    total(): number {
        return this.parentTotal() + this.child + this.#childSecret;
    }
}

const mixed = new Child();
console.log("mixed-total", mixed.total());
console.log("mixed-keys", JSON.stringify(Object.keys(mixed)));
console.log("mixed-names", JSON.stringify(Object.getOwnPropertyNames(mixed)));

class StaticCounter {
    static #count = 0;

    static increment(): number {
        this.#count += 1;
        return this.#count;
    }
}

console.log("static-compound", StaticCounter.increment());
