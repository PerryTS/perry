// Regression test for issue #235.
//
// Pre-fix, calling a class method with fewer args than it declares (relying
// on default param values for the trailing args) made the callee read
// uninitialized arg-register slots — typically a real heap pointer left
// over from a prior call's return state. Dereferencing that garbage
// inside the method body silently hung in the dispatch chain, manifesting
// as "second await never wakes" in the issue. Sometimes SIGSEGV, sometimes
// silent exit 0, depending on what bytes happened to be in the registers.
//
// The bug had two contributing parts:
//   1. The cross-module method DECLARE in `crates/perry-codegen/src/codegen.rs`
//      was hardcoded to 6 doubles ("safe upper bound"), but the call site
//      only passed `args.len() + 1` doubles. The trailing param-register
//      slots held garbage.
//   2. Local class method dispatch (both static + dynamic-dispatch tower in
//      `crates/perry-codegen/src/lower_call.rs`) didn't pad lowered_args to
//      match the method's declared arity either, so the same garbage-read
//      symptom appeared for fully-local class methods with default params.
//
// The fix:
//   - Add `method_param_counts: Vec<usize>` parallel to `method_names` on
//     `ImportedClass` so cross-module declares know the actual arity.
//   - Build a `method_param_counts: HashMap<(class, method), usize>` once
//     in `compile_module` covering BOTH local and imported classes; thread
//     through `CrossModuleCtx` → `FnCtx`.
//   - At every method-call dispatch site in lower_call.rs, look up the
//     implementor's arity (max across all overrides for the static-dispatch
//     virtual case) and pad `lowered_args` with TAG_UNDEFINED so the
//     callee's default-param desugaring fires correctly.
//
// The original failure required @perryts/mongodb (a real socket-resolved
// promise chain) to manifest reliably. This test exercises the same
// dispatch-tower bug shape with pure local code so it can run in CI.

// ─── 1. Local class with one default param, called missing it. ───
function caseLocalDefault(): void {
    class Calc {
        add(a: number, b: number = 10): number {
            return a + b;
        }
    }
    const c = new Calc();
    console.log("1a: " + c.add(5));      // expect 15 (b defaults to 10)
    console.log("1b: " + c.add(5, 7));   // expect 12 (b explicit)
}
caseLocalDefault();

// ─── 2. Local class, default-param defaulting to an OBJECT literal. ───
//    Pre-fix the trailing arg-register held a real heap pointer from a
//    prior call, so `options.x` returned that object's `x` field instead
//    of the default `{x: 99}.x`. Object-default trumps register-leftover.
function caseObjectDefault(): void {
    class Box {
        get(opts: { x: number } = { x: 99 }): number {
            return opts.x;
        }
    }
    const b = new Box();
    console.log("2a: " + b.get());            // expect 99 (default)
    console.log("2b: " + b.get({ x: 42 }));   // expect 42
}
caseObjectDefault();

// ─── 3. Local class with TWO default params. ───
//    Tests that padding extends to the full arity, not just the first
//    missing arg.
function caseTwoDefaults(): void {
    class Greeter {
        greet(name: string = "world", suffix: string = "!"): string {
            return "hello " + name + suffix;
        }
    }
    const g = new Greeter();
    console.log("3a: " + g.greet());                         // expect "hello world!"
    console.log("3b: " + g.greet("perry"));                  // expect "hello perry!"
    console.log("3c: " + g.greet("perry", "?"));             // expect "hello perry?"
}
caseTwoDefaults();

// ─── 4. Async method with default param + multiple awaits between. ───
//    Mirrors the original issue's shape — the method body has multiple
//    awaits, so the trailing-register garbage on entry was preserved
//    through the wait loop into the dispatch chain. Pre-fix this
//    silently hung; post-fix it completes and prints all 3 lines.
async function caseAsyncDefault(): Promise<void> {
    class Worker {
        async process(input: number, scale: number = 2): Promise<number> {
            await Promise.resolve();
            const intermediate = input * scale;
            await Promise.resolve();
            return intermediate;
        }
    }
    const w = new Worker();
    const a = await w.process(5);          // expect 10 (scale defaults to 2)
    console.log("4a: " + a);
    const b = await w.process(5, 3);       // expect 15 (scale explicit)
    console.log("4b: " + b);
    const c = await w.process(7);          // expect 14 (scale defaults to 2)
    console.log("4c: " + c);
}
asyncMain();

async function asyncMain(): Promise<void> {
    await caseAsyncDefault();
}

// ─── 5. Class method called via dynamic dispatch (receiver typed as the
//    class but referenced through a field that's typed Any). ───
//    Triggers the lower_call.rs dynamic-dispatch tower (idispatch.case0)
//    rather than the static dispatch path. Same padding requirement; the
//    fix wires both paths.
function caseDynamicDispatch(): void {
    class Adder {
        add(a: number, b: number = 100): number {
            return a + b;
        }
    }
    const wrapper: any = { adder: new Adder() };
    // wrapper.adder is typed Any → dispatch goes through the dynamic
    // class_id check tower. We pass only 1 arg; pre-fix the second arg
    // (b) was uninitialized, post-fix it defaults to 100.
    console.log("5a: " + wrapper.adder.add(5));    // expect 105
    console.log("5b: " + wrapper.adder.add(5, 7)); // expect 12
}
caseDynamicDispatch();

// ─── 6. Class method called inside a longer chain so the prior call's
//    return value is plausibly still in d2/xmm2 (the register slot the
//    missing arg would read). Pre-fix this often surfaced the bug as a
//    crash rather than a hang. ───
function caseAfterPriorCall(): void {
    class Helper {
        compute(x: number, modifier: number = 1): number {
            return x * modifier + 100;
        }
    }
    const h = new Helper();
    // Prior compute call seeds d2 with whatever value happens to be there.
    const seed = h.compute(2, 50);
    console.log("6a: " + seed);              // 200
    // This call leaves modifier defaulted; pre-fix d2 still held 50 from
    // the prior call's return-related register state on some codegen paths.
    const next = h.compute(3);
    console.log("6b: " + next);              // expect 103 (3*1 + 100)
}
caseAfterPriorCall();
