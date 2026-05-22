import os from "node:os";

try { console.log("getPriority self:", typeof os.getPriority()); } catch (err: any) { console.log("getPriority self:", err?.name, err?.code || "no-code"); }
try { os.getPriority(-999999); console.log("getPriority bad no throw"); } catch (err: any) { console.log("getPriority bad:", err?.name, err?.code || "no-code"); }
try { os.setPriority(999999 as any); console.log("setPriority one arg ok"); } catch (err: any) { console.log("setPriority one arg:", err?.name, err?.code || "no-code"); }
