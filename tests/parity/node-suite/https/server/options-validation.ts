import * as https from "node:https";

function accepts(value: any) {
  const server = https.createServer(value as any);
  console.log(
    value === null ? "null" : typeof value,
    server instanceof https.Server,
  );
  server.close();
}

function rejects(value: any) {
  try {
    const server = https.createServer(value as any);
    console.log(typeof value, "accepted");
    server.close();
  } catch (error: any) {
    console.log(typeof value, error.name, error.code);
  }
}

accepts(undefined);
accepts(null);
accepts({});
accepts(() => {});
rejects(false);
rejects(1);
rejects("options");
