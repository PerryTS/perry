// #1013 coverage fixture: cross-module async helpers that await an
// inner lookup and return a property read off the resolved value.
// This is the exact shape the reporter saw fail in gscmaster-api —
// same-file fixtures don't exercise it because the importer-side
// closure resolution lives in the same module as the helpers.

type User = { id: string; accessToken: string; plan: string };

async function lookupUser(userId: string): Promise<User | null> {
    // A trivial inner await to keep the helper async-shaped; the
    // outer `await Promise.all([getAccessToken(...), getUserPlan(...)])`
    // is what the regression targets.
    const users: Record<string, User> = {
        u1: { id: "u1", accessToken: "tok-256-chars-aaaaaaaaaaaaa", plan: "pro" },
    };
    await Promise.resolve(null);
    return users[userId] ?? null;
}

export async function getAccessToken(userId: string): Promise<string | null> {
    const user = await lookupUser(userId);
    if (!user) return null;
    // Property-read return — the original repro's smoking gun. The
    // returned value crosses an async-step frame boundary; before #1007
    // the property read landed in a stale NaN-boxed slot at the
    // destructure site.
    return user.accessToken;
}

export async function getUserPlan(userId: string): Promise<{ plan: string } | null> {
    const user = await lookupUser(userId);
    if (!user) return null;
    return { plan: user.plan };
}
