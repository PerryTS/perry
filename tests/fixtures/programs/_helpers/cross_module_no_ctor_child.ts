// Cross-module child for v0.5.758 test: extends Parent2 (in another file),
// has field initializers (config, arrow), no own constructor.
import { Parent2 } from "./cross_module_no_ctor_parent.ts";

export class Child extends Parent2 {
    config: any = { x: 1 };
    arrow = (): string => "from-arrow";
}
