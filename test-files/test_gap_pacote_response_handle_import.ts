// A userland fetch returns a JavaScript Response, like npm-registry-fetch.
// Its inherited json() method must not receive native Response handle dispatch.
import { fetch } from "./_helpers/pacote_user_fetch.ts";

async function main() {
  const response = await fetch();
  console.log((await response.json()).name);
}
main().catch(error => console.log(error.message));
