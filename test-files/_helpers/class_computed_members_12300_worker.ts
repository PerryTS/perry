import { parentPort } from "node:worker_threads";
import { C, exercise } from "./class_computed_members_12300.ts";
parentPort!.postMessage("worker " + exercise(new C()));
