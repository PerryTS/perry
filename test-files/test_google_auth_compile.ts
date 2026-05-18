// Compile-smoke for `@perryts/google-auth` (issue #674).
//
// References each of the three FFI entry points and prints the
// resolved JSON. The MVP stub resolves synchronously with
// `{ success: false, error: "..." }` on every platform, so the
// program exits 0 with three printed JSON lines.

import {
  js_google_auth_sign_in,
  js_google_auth_silent_sign_in,
  js_google_auth_sign_out,
} from "@perryts/google-auth";

async function main() {
  const a = await js_google_auth_sign_in();
  console.log("sign_in:", a);

  const b = await js_google_auth_silent_sign_in();
  console.log("silent_sign_in:", b);

  const c = await js_google_auth_sign_out();
  console.log("sign_out:", c);
}

main();
