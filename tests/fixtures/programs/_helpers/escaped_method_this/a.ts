// Classes whose `get` leaves its module as a value (test_gap_escaped_method_this).
export class Ret {
    raw: any;
    constructor() { this.raw = 1; }
    get() { return this.raw; }
    static box() {
        function Box(this: any) { this.get = Ret.prototype.get; }
        return new (Box as any)();
    }
}

export class Fld {
    raw: any;
    constructor() { this.raw = 2; }
    get() { return this.raw; }
    static put(o: any) { o.get = Fld.prototype.get; }
}

export class Arr {
    raw: any;
    constructor() { this.raw = 3; }
    get() { return this.raw; }
    static arr() { return [Arr.prototype.get]; }
}

export class Proto {
    raw: any;
    constructor() { this.raw = 4; }
    get() { return this.raw; }
}

export class Inst {
    raw: any;
    constructor() { this.raw = 5; }
    get() { return this.raw; }
}

export class Lit {
    raw: any;
    constructor() { this.raw = 6; }
    get() { return this.raw; }
}
export const box: any = { get: Lit.prototype.get };

export class St {
    raw: any;
    fn: any;
    constructor() { this.raw = 7; this.fn = St.prototype.get; }
    get() { return this.raw; }
}
