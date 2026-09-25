// zod 4.6.5 re-exports `validate`/`validateAsync` from core/index.ts, whose
// `export *` sources also include a private `validateAsync` (core/schemas.ts).
import { z } from "zod";

const User = z.object({ name: z.string(), age: z.number().int().min(0) });

const ok = User.safeParse({ name: "Ada", age: 36 });
console.log("safeParse ok:", ok.success, JSON.stringify(ok.success ? ok.data : null));

const bad = User.safeParse({ name: 1, age: -1 });
console.log("safeParse error:", bad.success);
if (!bad.success) {
  for (const issue of bad.error.issues) {
    console.log(" ", issue.code, issue.path.join("."));
  }
}

console.log("z.core:", typeof z.core, typeof z.core.$constructor);
console.log("validate:", typeof z.validate, typeof z.validateAsync);

async function main(): Promise<void> {
  const parsed = await User.parseAsync({ name: "Grace", age: 85 });
  console.log("parseAsync:", JSON.stringify(parsed));
  try {
    await User.parseAsync({ name: "Linus" });
  } catch (err) {
    console.log("parseAsync error:", err instanceof z.ZodError);
  }
}

main();
