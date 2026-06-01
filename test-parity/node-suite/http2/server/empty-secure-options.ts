import * as http2 from "node:http2";

const bare = http2.createSecureServer({});
console.log("http2 empty secure options typeof:", typeof bare);
bare.close();

const withListener = http2.createSecureServer({}, (_req: any, _res: any) => {});
console.log("http2 empty secure options listener typeof:", typeof withListener);
withListener.close();
