import assert from "node:assert";

try { assert.match("hello world", /world/); console.log("match pass"); } catch (err: any) { console.log("match pass err:", err?.operator); }
try { assert.doesNotMatch("hello", /world/); console.log("doesNotMatch pass"); } catch (err: any) { console.log("doesNotMatch pass err:", err?.operator); }
try { assert.match("hello", /world/); console.log("match fail no throw"); } catch (err: any) { console.log("match fail:", err?.name, err?.operator); }
try { assert.match(123 as any, /x/); console.log("non-string no throw"); } catch (err: any) { console.log("non-string:", err?.name, err?.code || err?.operator); }
