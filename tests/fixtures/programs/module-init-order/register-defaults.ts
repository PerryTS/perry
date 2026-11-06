// Exercises the init-order bug from issue #32: top-level code in this
// file must run AFTER registry.ts's init (which allocates the arrays),
// otherwise `register()` pushes into a module global that's still 0.0
// and the data is unreachable.
//
// Also exercises `import * as O` — `O.OID_A` must resolve through the
// namespace to oids.ts's getter, not fall through to a bogus
// `js_object_get_field_by_name(TAG_TRUE, "OID_A")` lookup.
import { register } from './registry';
import * as O from './oids';

export function registerAll(): void {
    register(O.OID_A, "codec_a");
    register(O.OID_B, "codec_b");
    register(O.OID_C, "codec_c");
}

registerAll();
