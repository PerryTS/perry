import { readExportedClass } from "./issue_12007_cycle_b.ts";

try {
  readExportedClass();
  console.log("cycle-before", "no throw");
} catch (error: any) {
  console.log("cycle-before", error && error.name);
}

export const ExportedClass = class {};
export { readExportedClass };
