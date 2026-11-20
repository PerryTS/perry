import type { User } from "./model.ts";
import { prefix } from "./model.ts";

const user: User = { name: "Ada" };
console.log(`type-import:${prefix}:${user.name}`);
