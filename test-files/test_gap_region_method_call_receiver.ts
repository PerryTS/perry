// A loop region over a receiver whose exact class is proven, with a method
// call on that receiver in the body. The call's dispatch keeps the proven
// direct route, and whatever the method writes into the fields the region
// reads (a Number, a string, an object with valueOf) is seen by the next
// iteration exactly as node sees it.
const SINK: any[] = [];

class C {
    a: number;
    d: number;
    constructor(a: number, d: number) {
        this.a = a;
        this.d = d;
    }
    m(): number {
        return this.a;
    }
    poke(k: number): void {
        if (k === 4) (this as any).a = "s";
        if (k === 7) (this as any).a = { valueOf() { return 100; } };
        if (k === 9) this.a = 2;
    }
}

function viaMethod(n: number): any {
    const o: any = new C(3, 0);
    let h = 0.0;
    for (let k = 0; k < n; k++) {
        o.d = k;
        h += o.m();
    }
    if (h < 0) return o;
    return h + o.d;
}

function pokeThenRead(n: number): any {
    const o: any = new C(1, 0);
    let h: any = 0.0;
    for (let k = 0; k < n; k++) {
        o.d = k;
        h += o.a;
        o.poke(k);
    }
    SINK.push(o);
    return h;
}

class D extends C {
    m(): number {
        return this.a * 10;
    }
}

function subclass(n: number): any {
    const o: any = new D(2, 0);
    let h = 0.0;
    for (let k = 0; k < n; k++) {
        o.d = k;
        h += o.m();
    }
    SINK.push(o);
    return h;
}

console.log("method", viaMethod(10), viaMethod(1000));
console.log("poke", String(pokeThenRead(12)));
console.log("subclass", subclass(5));
console.log("sink", SINK.length);
