// Refs #4508: `Array.prototype.push` with more than one argument (two
// spreads, or a spread combined with another element) failed native
// codegen when the call was inside a class method or getter body. The
// HIR lowered the whole arg list into a single `push_spread`
// NativeMethodCall, and the codegen arm bailed with
// "array.push_spread expects exactly 1 arg, got 2", rejecting the whole
// module. The same statement at module top level (local-array receiver)
// already worked because that path decomposed multi-arg push into a
// Sequence of single-arg pushes. Found in the wild in zod's
// `ParseInputLazyPath.get path()`.
//
// Fix: decompose the multi-arg/property-receiver case the same way —
// into per-arg single native pushes, choosing spread vs single from the
// original AST spread flag. Output is byte-for-byte against Node.

class Path {
    _path = [1, 2];
    _key: number | number[];
    _cached: number[] = [];
    constructor(k: number | number[]) {
        this._key = k;
    }
    get path(): number[] {
        if (Array.isArray(this._key)) {
            this._cached.push(...this._path, ...this._key); // two spreads
        } else {
            this._cached.push(...this._path, this._key); // spread + element
        }
        return this._cached;
    }
}
console.log(new Path([3, 4]).path.join(",")); // 1,2,3,4
console.log(new Path(9).path.join(",")); // 1,2,9

// element-first then spread then element, inside a method body.
// (Pre-fix this also mis-routed: only the FIRST arg was checked for a
// spread, so a leading non-spread arg sent the call to `push_single`,
// which would have treated the trailing `...a` as a single element.)
class M {
    out: number[] = [];
    run(a: number[]) {
        this.out.push(0, ...a, 7);
        return this.out;
    }
}
console.log(new M().run([5, 6]).join(",")); // 0,5,6,7

// no-spread multi-arg push on a property receiver still works
class N {
    out: number[] = [];
    fill() {
        this.out.push(1, 2, 3);
        return this.out;
    }
}
console.log(new N().fill().join(",")); // 1,2,3
