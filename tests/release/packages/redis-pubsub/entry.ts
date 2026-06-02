import { createClient } from "redis";

async function main(): Promise<void> {
    const url = process.env.REDIS_URL ?? "redis://127.0.0.1:18903";

    const sub = createClient({ url });
    const pub = createClient({ url });

    await sub.connect();
    await pub.connect();

    const received: string = await new Promise<string>((resolve) => {
        sub.subscribe("greetings", (message: string) => resolve(message));
        // Tiny delay so the subscribe round-trip lands before publish.
        setTimeout(() => { pub.publish("greetings", "hello"); }, 50);
    });

    console.log(`received=${received}`);

    await sub.unsubscribe("greetings");
    await sub.quit();
    await pub.quit();
}

main();
