// Exported class in module A
export class MyClass {
    public value: number;

    constructor(v: number) {
        this.value = v;
    }

    getValue(): number {
        return this.value;
    }
}
