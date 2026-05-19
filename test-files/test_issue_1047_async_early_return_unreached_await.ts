// #1047: async fn with an early `return X` followed by an unreached
// `await` infinite-looped because the state body's user-level returns
// were only rewritten on `StateExit::Done` states. State 1 here ends
// in a Yield (the unreached `await insertNew(kid)`), so the original
// `return existing.kid` reached next() as a raw Return — the
// AsyncStepChain caller treated the non-iter-result as `done=false`
// and re-entered the same state with state_id unchanged.

type Row = { kid: string; algorithm: string; status: string };

async function findExisting(): Promise<Row | null> {
    return { kid: "the-kid", algorithm: "aes-256-gcm", status: "active" };
}

async function insertNew(_k: string): Promise<void> {
    // unreached
}

async function getKid(): Promise<string> {
    const existing = await findExisting();
    if (existing) return existing.kid;
    const kid = `dek-${Date.now().toString(36)}`;
    await insertNew(kid);
    return kid;
}

const k = await getKid();
console.log("k:", k, " typeof:", typeof k);
