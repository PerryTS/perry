// Prints a cloned value in a form that shows its kinds, bytes, holes and
// shared references (`#n` names the n-th object met), the same in Node and
// Perry.
export function describe(value: any): string {
    const ids = new Map<any, number>();
    const id = (o: any): string => {
        if (ids.has(o)) return "#" + ids.get(o);
        ids.set(o, ids.size);
        return "";
    };
    const go = (v: any): string => {
        if (v === null) return "null";
        if (typeof v === "undefined") return "undefined";
        if (typeof v === "number") return Object.is(v, -0) ? "-0" : String(v);
        if (typeof v === "bigint") return v + "n";
        if (typeof v === "string") return JSON.stringify(v);
        if (typeof v === "boolean") return String(v);
        if (typeof v !== "object") return "<" + typeof v + ">";
        const seen = id(v);
        if (seen) return seen;
        if (v instanceof ArrayBuffer) return "AB(" + v.byteLength + ")[" + Array.from(new Uint8Array(v)).join(",") + "]";
        if (v instanceof DataView) {
            const buf = id(v.buffer) || "AB" + v.buffer.byteLength;
            const bytes: number[] = [];
            for (let i = 0; i < v.byteLength; i++) bytes.push(v.getUint8(i));
            return "DataView(off=" + v.byteOffset + ",buf=" + buf + ")[" + bytes.join(",") + "]";
        }
        if (ArrayBuffer.isView(v)) {
            const t: any = v;
            const buf = id(t.buffer) || "AB" + t.buffer.byteLength;
            return t.constructor.name + "(off=" + t.byteOffset + ",buf=" + buf + ")[" + Array.from(t).join(",") + "]";
        }
        if (Array.isArray(v)) {
            const parts: string[] = [];
            for (let i = 0; i < v.length; i++) parts.push(i in v ? go(v[i]) : "<hole>");
            return "[" + parts.join(",") + "]";
        }
        if (v instanceof Date) return "Date(" + v.getTime() + ")";
        if (v instanceof RegExp) return "RegExp(/" + v.source + "/" + v.flags + " last=" + v.lastIndex +
            " own=" + Object.getOwnPropertyNames(v).join(",") + " ctor=" + v.constructor.name +
            " keys=" + Object.keys(v).join(",") + ")";
        if (v instanceof Map) return "Map{" + Array.from(v).map(([k, x]) => go(k) + "=>" + go(x)).join(",") + "}";
        if (v instanceof Set) return "Set{" + Array.from(v).map(go).join(",") + "}";
        if (v instanceof Error) {
            return v.name + "(" + JSON.stringify(v.message) + " ctor=" + v.constructor.name + " code=" + (v as any).code +
                " stack=" + typeof v.stack + " cause=" + ("cause" in v ? go((v as any).cause) : "none") + ")";
        }
        const ctor = Object.getPrototypeOf(v) === null ? "null" : v.constructor.name;
        return ctor + "{" + Object.keys(v).map((k) => k + ":" + go(v[k])).join(",") + "}";
    };
    return go(value);
}
