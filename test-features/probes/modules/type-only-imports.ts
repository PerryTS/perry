import type { MatrixPayload } from "./support/type-only-values.ts";
import { render } from "./support/type-only-values.ts";

const payload: MatrixPayload = { label: "modules/type-only-imports", value: 5 };
console.log(render(payload));
