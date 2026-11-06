import { WebSocketServer, WebSocket } from "ws";

async function main(): Promise<void> {
    const port = 18901;
    const wss = new WebSocketServer({ port });

    wss.on("connection", (sock: WebSocket) => {
        sock.on("message", (data: Buffer) => {
            sock.send(`echo:${data.toString()}`);
        });
    });

    // Wait for the server to be listening before connecting.
    await new Promise<void>((resolve) => {
        wss.on("listening", () => resolve());
    });

    const client = new WebSocket(`ws://127.0.0.1:${port}`);
    const reply: string = await new Promise<string>((resolve, reject) => {
        client.on("open", () => client.send("hello"));
        client.on("message", (data: Buffer) => resolve(data.toString()));
        client.on("error", (err: Error) => reject(err));
    });

    console.log(`reply=${reply}`);

    client.close();
    wss.close();
}

main();
