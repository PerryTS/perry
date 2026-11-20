import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { chdir, cwd } from "node:process";
import { createTracing, getEnabledCategories } from "node:trace_events";

const parent = cwd();
const temporary = mkdtempSync(join(tmpdir(), "perry-trace-mutation-"));

try {
  chdir(temporary);
  const categories = ["beta", "alpha", "beta"];
  const tracing = createTracing({ categories });

  categories[0] = "changed-before-enable";
  categories.push("appended-before-enable");
  console.log("property before enable:", tracing.categories);

  tracing.enable();
  console.log("enabled categories:", String(getEnabledCategories()));

  categories[1] = "changed-after-enable";
  categories.length = 1;
  console.log("property after mutation:", tracing.categories);
  console.log("enabled after mutation:", String(getEnabledCategories()));

  tracing.disable();
  console.log("after disable:", String(getEnabledCategories()));

  categories[0] = 123 as any;
  console.log("post-construction number coercion:", tracing.categories);
  categories[0] = Symbol("category") as any;
  try {
    console.log("post-construction symbol coercion:", tracing.categories);
  } catch (error: any) {
    console.log(
      "post-construction symbol coercion: THROW",
      error.name,
      String(error.code),
    );
  }
} finally {
  chdir(parent);
  rmSync(temporary, { recursive: true, force: true });
}
