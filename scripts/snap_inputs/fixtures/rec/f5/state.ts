import { threadId } from "node:worker_threads";
export const rec = { evals: 0, tid: -1 };
rec.evals++;
rec.tid = threadId;
