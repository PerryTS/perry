//! `crypto.randomUUID()` inside an async body on the web target, which is emitted as
//! JavaScript.

use super::run_web_source;

// The namespace and named imports and the global `crypto` each give a fresh version-4
// UUID before and after an `await`, as in a synchronous body.
#[test]
fn random_uuid_in_an_async_body_is_a_v4_uuid() {
    assert_eq!(
        run_web_source(
            r#"
import * as ns from "crypto";
import { randomUUID } from "node:crypto";
const v4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
async function ids(): Promise<string> {
  const a = ns.randomUUID();
  await 0;
  const b = randomUUID();
  const c = crypto.randomUUID();
  return [v4.test(a), v4.test(b), v4.test(c), a !== b].join(",");
}
ids().then((v) => console.log(v), (e) => console.log("threw " + e.message));
"#
        ),
        "true,true,true,true"
    );
}
