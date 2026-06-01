import * as https from "node:https";

const bare = https.createServer({});
console.log("https empty options typeof:", typeof bare);
bare.close();

const withListener = https.createServer({}, (_req: any, _res: any) => {});
console.log("https empty options listener typeof:", typeof withListener);
withListener.close();
