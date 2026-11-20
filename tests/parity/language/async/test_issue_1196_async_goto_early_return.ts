// #1196: async fn with an early `return X` inside an `if`, immediately
// followed by an unreached `for ... await ...` loop wedged the runtime —
// the state body holding the early-return exited via StateExit::Goto
// (to the loop's condition state), and the Goto arm of the state-machine
// case builder didn't apply `rewrite_returns_as_done` /
// `prepend_done_before_returns`. The early-return became
// `[Expr(value), LabeledBreak]` with IterResult never set, so the
// post-step code re-chained the step closure onto the previous yield's
// resolved value and looped forever, allocating ~5 MB of arena per call.
//
// Companion to test_issue_1047 (which fixed the StateExit::Yield arm).

async function withCallback<T>(fn: () => Promise<T>): Promise<T> {
    return fn();
}

async function inner(): Promise<number[]> {
    return withCallback(async () => {
        const rows = await new Promise<number[]>((r) => r([]));
        if (rows.length === 0) return [];
        for (const id of rows) {
            await new Promise<void>((r) => r());
            void id;
        }
        return rows;
    });
}

const result = await inner();
console.log("result length:", result.length, " is array:", Array.isArray(result));
