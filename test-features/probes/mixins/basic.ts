type Constructor<T = {}> = new (...args: any[]) => T;

function Tagged<TBase extends Constructor>(Base: TBase) {
    return class extends Base {
        tag() {
            return "tag";
        }
    };
}

class Item {
    name() {
        return "item";
    }
}

const Mixed = Tagged(Item);
const value = new Mixed();
console.log(`mixin:${value.name()}:${value.tag()}`);
