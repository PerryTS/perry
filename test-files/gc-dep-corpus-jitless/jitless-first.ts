// #7803 EXPERIMENT. Must run BEFORE any module that builds a schema: zod
// captures `const jit = !core.globalConfig.jitless` when the $ZodObject is
// constructed (core/schemas.ts:2007), so a `config()` call in main.ts's body
// is already too late for the schemas alerts/orgs/scans build at import time.
import * as z from "../../node_modules/zod/src/index.js";
z.config({ jitless: true });
