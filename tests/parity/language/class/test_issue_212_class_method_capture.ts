// Regression test for issue #212.
//
// Pre-fix, a class declared inside a function whose method body referenced
// a local of the enclosing function failed to compile with
// `local not in scope` because `lower_class_method` emitted the method as a
// stand-alone function with no capture wiring.
//
// The fix in `crates/perry-hir/src/lower_decl.rs` adds hidden
// `__perry_cap_<id>` instance fields per captured outer local, prepends
// `let id = this.__perry_cap_<id>` to each capturing method body so the
// existing `LocalGet(id)` resolves to a method-local slot at codegen, and
// extends the constructor with one synthesized param per captured id +
// `this.__perry_cap_<id> = LocalGet(id)` assignments. The `Expr::New`
// lowering at every construction site appends `LocalGet(id)` per captured
// id, so the outer scope's current value is snapshotted onto each instance.
//
// Mutation note: `LocalSet(id, ...)` inside a method writes only to the
// method-local slot, not back to the outer scope. The common case — a
// closure over a reference type like an array (`captured.push(...)`) or
// object (`obj.x = ...`) — works because the method-local copy and the
// outer binding hold the same reference. We don't test primitive
// reassignment here because the JS-spec semantic is divergent and out of
// scope for #212's headline ask (compile-fail → compile + correct for the
// reference-mutation case).

// 1. The exact repro from the issue.
function basic() {
    const captured: string[] = [];
    class C {
        log(s: string): void {
            captured.push(s);
        }
    }
    const c = new C();
    c.log("hi");
    c.log("world");
    console.log("basic: " + captured.join(", "));
}
basic();

// 2. Multiple captures + a method param that shadows an outer local name.
function multiCapture() {
    const tag = "[m]";
    const log: string[] = [];
    class M {
        emit(s: string): void {
            log.push(tag + " " + s);
        }
    }
    const m = new M();
    m.emit("one");
    m.emit("two");
    console.log("multi: " + log.join(" / "));
}
multiCapture();

// 3. A user-written constructor that itself references a captured local.
//    Verifies the synthesized capture-param + assignment work alongside
//    user constructor params.
function withConstructor() {
    const events: string[] = [];
    class E {
        prefix: string;
        constructor(p: string) {
            this.prefix = p;
            events.push("ctor:" + p);
        }
        record(s: string): void {
            events.push(this.prefix + ":" + s);
        }
    }
    const a = new E("a");
    const b = new E("b");
    a.record("hello");
    b.record("world");
    console.log("ctor: " + events.join(", "));
}
withConstructor();

// 4. Each `new C()` snapshots the current outer scope: two invocations of
//    the enclosing function produce two independent capture chains.
function makeCounter(label: string): { tick: () => void; report: () => void } {
    const log: string[] = [];
    class T {
        n: number = 0;
        tick(): void {
            this.n += 1;
            log.push(label + ":" + this.n);
        }
        report(): void {
            console.log("counter " + label + ": " + log.join(","));
        }
    }
    const t = new T();
    return {
        tick: () => t.tick(),
        report: () => t.report(),
    };
}
const a = makeCounter("a");
const b = makeCounter("b");
a.tick();
a.tick();
b.tick();
a.tick();
b.tick();
a.report();
b.report();

// 5. Nested closure inside a capturing method. The method-local rebind
//    `let captured = this.__perry_cap_<id>` puts the outer id back into
//    method scope, which the inner arrow can then capture via the
//    standard closure machinery — no second wiring needed.
//    NB: method name avoids `forEach` so the call doesn't get
//    intercepted by the array-method builtin dispatch.
function withNestedClosure() {
    const out: string[] = [];
    class N {
        emitAll(items: string[]): void {
            items.forEach((it: string) => out.push("> " + it));
        }
    }
    const n = new N();
    n.emitAll(["x", "y", "z"]);
    console.log("nested: " + out.join(" "));
}
withNestedClosure();

// 6. The #154 dispose-hook pattern — pre-fix this caused a silent drop of
//    the dispose method (compile succeeded but no disposal output). Now
//    the dispose hook fires properly because the same capture rewrite
//    handles `[Symbol.dispose]` bodies referencing outer locals.
function disposalScope(): void {
    const disposed: string[] = [];
    class R {
        label: string;
        constructor(l: string) {
            this.label = l;
        }
        [Symbol.dispose](): void {
            disposed.push(this.label);
        }
    }
    {
        using r1 = new R("first");
        using r2 = new R("second");
    }
    console.log("dispose: " + disposed.join(", "));
}
disposalScope();

// 7. `class Derived extends Base` where BOTH classes have methods that
//    capture the SAME outer local. The hidden `__perry_cap_<id>` field
//    is declared once on the parent (the child's lowering detects the
//    parent already has it via `lookup_class_field_names` and skips
//    re-declaring) — otherwise the keys array would carry two same-
//    named entries at different offsets and the parent's method would
//    read its index while the child's ctor wrote to the child's index.
//    The child's synthesized ctor still takes the capture as a param
//    and the PropertySet by-name lookup writes to the (single) parent-
//    declared field at runtime.
function inheritedSharedCapture() {
    const log: string[] = [];
    class SharedBase {
        baseHit(): void { log.push("base"); }
    }
    class SharedDerived extends SharedBase {
        derivedHit(): void { log.push("derived"); }
    }
    const d = new SharedDerived();
    d.baseHit();
    d.derivedHit();
    console.log("inherit-shared: " + log.join(","));
}
inheritedSharedCapture();

// 8. `class Derived extends Base` where Base captures one outer local
//    and Derived captures a DIFFERENT one. Without parent-capture
//    propagation, the child's synthesized ctor wouldn't take the
//    parent's capture as a param, so the parent's field would never be
//    initialized — its method would read a stale slot (here, an
//    undefined value crashed the join). The lowering unions the
//    parent's `class_captures` registry into the child's captures_vec
//    so the New site passes args for both.
function inheritedDisjointCapture() {
    const baseLog: string[] = [];
    const derivedLog: string[] = [];
    class DisjointBase {
        baseHit(): void { baseLog.push("b"); }
    }
    class DisjointDerived extends DisjointBase {
        derivedHit(): void { derivedLog.push("d"); }
    }
    const d = new DisjointDerived();
    d.baseHit();
    d.derivedHit();
    d.baseHit();
    console.log("inherit-base: " + baseLog.join(","));
    console.log("inherit-derived: " + derivedLog.join(","));
}
inheritedDisjointCapture();

// 9. Captured-primitive reassignment via setter. The setter writes to
//    `stored` (an outer-fn primitive) and the next getter call must
//    read the freshly-written value, not the field's snapshot. The
//    `lower_class_decl` rewrite wraps `LocalSet(captured_id, v)` in a
//    `Sequence` that also writes through to `this.__perry_cap_<id>`,
//    so subsequent method calls re-reading the field see the latest
//    value. (Mutations to the OUTER scope's binding still don't
//    propagate — that's the documented JS divergence.)
function setterCapture() {
    let stored = "init";
    class Box {
        get value(): string { return stored; }
        set value(v: string) { stored = v; }
    }
    const b = new Box();
    console.log("setter1: " + b.value);
    b.value = "hello";
    console.log("setter2: " + b.value);
    b.value = "world";
    console.log("setter3: " + b.value);
}
setterCapture();

// 10. Async methods that capture an outer local. The capture machinery
//    runs on the lowered (post-async-transform) method body the same as
//    sync methods, so async closures over outer arrays work too. Last
//    test in the file because async logs drain on microtask flush
//    after all synchronous statements complete — Node and Perry both
//    flush this output last.
async function asyncCapture(): Promise<void> {
    const log: string[] = [];
    class Worker {
        async run(items: string[]): Promise<void> {
            for (const it of items) {
                log.push("done:" + it);
            }
        }
    }
    const w = new Worker();
    await w.run(["a", "b", "c"]);
    console.log("async: " + log.join(","));
}
asyncCapture();
