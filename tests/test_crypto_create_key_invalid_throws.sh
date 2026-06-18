#!/usr/bin/env bash
set -euo pipefail

# Node's `crypto.createPrivateKey` / `createPublicKey` THROW on input that is
# not valid key material (e.g. a plain non-PEM string), rather than producing a
# KeyObject with `type === undefined`. jsonwebtoken's sign()/verify() rely on
# this: they `try { createPrivateKey(secret) } catch { createSecretKey(...) }`,
# so a string HMAC secret must make createPrivateKey/createPublicKey throw to
# reach the createSecretKey fallback. Pre-fix perry returned the string as a
# bogus key (type undefined), so HS256 signing reported "secretOrPrivateKey
# must be a symmetric key" and verify reported "invalid algorithm".

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$REPO_ROOT/target/release/perry}}"
if [[ ! -x "$PERRY" ]]; then PERRY="$REPO_ROOT/target/debug/perry"; fi
if [[ ! -x "$PERRY" ]]; then
    echo "SKIP: perry binary not found (build with cargo build -p perry)"
    exit 0
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

cat >"$TMPDIR/f.ts" <<'TS'
import { createPrivateKey, createPublicKey, createSecretKey } from "crypto";

let priv = false;
try { createPrivateKey("test-secret-key"); } catch (_) { priv = true; }
if (!priv) throw new Error("createPrivateKey should throw on a non-PEM string");

let pub = false;
try { createPublicKey("test-secret-key"); } catch (_) { pub = true; }
if (!pub) throw new Error("createPublicKey should throw on a non-PEM string");

// The legitimate symmetric-key fallback still works and is type 'secret'.
const sk: any = createSecretKey(Buffer.from("test-secret-key"));
if (sk.type !== "secret") throw new Error("createSecretKey type: " + sk.type);

console.log("OK");
TS

OUT="$("$PERRY" run "$TMPDIR/f.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }
if ! grep -q "^OK$" <<<"$OUT"; then echo "FAIL: expected OK, got:"; echo "$OUT"; exit 1; fi
echo "PASS: createPrivateKey/createPublicKey throw on invalid key material"
