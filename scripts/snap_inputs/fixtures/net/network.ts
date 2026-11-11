import { createServer } from "node:net";
const server=createServer();
server.listen(0,"127.0.0.1");
const address=server.address();
server.close();
export { address };
