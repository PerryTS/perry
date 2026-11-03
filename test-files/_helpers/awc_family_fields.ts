export class VariantFields {
    constructor(kind: number, value: any) {
        this.kind = kind;
        this.value = value;
    }
}
export function makeX() {
    const o = new VariantFields(10, "x-value");
    o.x = o.value + ":" + o.kind;
    return o;
}
export function makeY() {
    const o = new VariantFields(20, "y-value");
    o.y = o.value + ":" + o.kind;
    return o;
}
