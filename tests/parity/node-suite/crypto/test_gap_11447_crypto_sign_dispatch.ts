// #11447: CJS/captured crypto constructors must share the direct-call helpers.
import run from './fixtures/crypto_sign_dispatch/signer.cjs';
run();
