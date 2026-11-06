import { Effect } from "effect";

const mapped = Effect.succeed(21).pipe(
    Effect.map((value: number) => value * 2),
);

const program = Effect.gen(function* () {
    const label = yield* Effect.succeed("effect");
    const value = yield* mapped;
    return `${label}:${value}`;
});

async function main() {
    const asyncResult = await Effect.runPromise(program);
    const syncResult = Effect.runSync(
        Effect.succeed("sync").pipe(Effect.map((value: string) => `${value}:ok`)),
    );

    console.log(`runPromise=${asyncResult}`);
    console.log(`runSync=${syncResult}`);
}

main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
});
