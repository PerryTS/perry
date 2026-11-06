const env: any = process.env;
if (typeof env !== "object" || env === null) throw new Error("typeof process.env: " + typeof env);
if (env.PATH !== process.env.PATH) throw new Error("whole-object member read diverged");
if (Object.keys(env).length < 1) throw new Error("no keys");
function count(o: Record<string, string>): number { return Object.keys(o).length; }
if (count(process.env as any) < 1) throw new Error("passed-whole count 0");
console.log("OK");
