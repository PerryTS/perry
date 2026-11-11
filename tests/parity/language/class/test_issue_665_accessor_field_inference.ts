// Regression test for #665 (https://github.com/PerryTS/perry/issues/665),
// final residual case: when a class declares `set X(v)` and the
// constructor body writes `this.X = ...`, the HIR ctor-body
// field-inference pass mis-categorised `X` as an own data field. That
// allocated a `js_object_set_field_by_name`-visible inline slot
// (surfaced in `Object.keys`) which shadowed the inherited accessor
// when a subclass instance's `.X` was read across modules. The
// surfaced symptom in the rate-limiter-flexible bisect was
// `limiter.points: 0` (own-data slot won lookup) instead of `7` (via
// `get points()`), even though the setter literally ran and
// `_points` got the correct value of 7.
//
// Fix (v0.5.859) in `crates/perry-hir/src/lower_decl.rs`: before the
// `this.X = Y` field-inference loop, collect the union of own + inherited
// accessor names and skip the `fields.push` when the candidate matches.

class Abstract {
    constructor(opts: any = {}) {
        // @ts-ignore — bare `this.points` is intentional, exercises the
        // ctor-body field-inference path. Pre-fix this added `points` as
        // an own data field alongside `_points`.
        this.points = opts.points;
        // @ts-ignore
        this.duration = opts.duration;
    }
    // @ts-ignore
    get points(): any {
        return (this as any)._points;
    }
    // @ts-ignore
    set points(v: any) {
        (this as any)._points = v >= 0 ? v : 4;
    }
    // @ts-ignore
    get duration(): any {
        return (this as any)._duration;
    }
    // @ts-ignore
    set duration(v: any) {
        (this as any)._duration = typeof v === "undefined" ? 1 : v;
    }
}

class Memory extends Abstract {
    _mem: any;
    constructor(opts: any = {}) {
        super(opts);
        this._mem = {};
    }
    consume(key: string): string {
        return "consume:" + key + ":" + (this as any).points;
    }
}

const m = new Memory({ points: 7, duration: 60 });

// Reads should dispatch through the getters on the parent prototype,
// not return the spurious-slot's stale 0. Pre-fix Perry printed
// `points: 0` because the auto-inferred own-data slot shadowed
// `get points()`. Post-fix the getter dispatch wins.
console.log("points:", (m as any).points);
console.log("duration:", (m as any).duration);
console.log("consume:", m.consume("ip"));

// Spurious own-data slots for accessor names must NOT exist. Check
// set-membership (insertion order across Perry and Node may differ
// for class instances allocated through cjs-wrapped hoisted classes,
// and `.sort()` on the returned array runs into a separate
// keys-array-reference bug — so test via includes() instead).
const keys = Object.keys(m as any);
console.log("has-points-slot:", keys.includes("points"));
console.log("has-duration-slot:", keys.includes("duration"));
console.log("has-_points-slot:", keys.includes("_points"));
console.log("has-_duration-slot:", keys.includes("_duration"));
console.log("has-_mem-slot:", keys.includes("_mem"));
console.log("key-count:", keys.length);
