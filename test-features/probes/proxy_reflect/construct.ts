class Thing {
    value: string;

    constructor(value: string) {
        this.value = value;
    }
}

const Wrapped = new Proxy(Thing, {
    construct(target, args, newTarget) {
        const instance = Reflect.construct(target, args, newTarget);
        instance.value = `${instance.value}:proxy`;
        return instance;
    },
});

console.log(`proxy:${new (Wrapped as any)("x").value}`);
