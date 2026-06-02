import axios from "axios";
import { createServer, IncomingMessage, ServerResponse } from "node:http";

async function main(): Promise<void> {
    const port = 18902;
    const server = createServer((req: IncomingMessage, res: ServerResponse) => {
        if (req.url === "/json") {
            res.statusCode = 200;
            res.setHeader("content-type", "application/json");
            res.end(JSON.stringify({ ok: true, path: req.url }));
        } else {
            res.statusCode = 404;
            res.end("nope");
        }
    });

    await new Promise<void>((resolve) => {
        server.listen(port, () => resolve());
    });

    const r = await axios.get(`http://127.0.0.1:${port}/json`);
    console.log(`status=${r.status}`);
    console.log(`data.ok=${r.data.ok}`);
    console.log(`data.path=${r.data.path}`);

    server.close();
}

main();
