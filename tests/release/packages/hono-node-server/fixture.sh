#!/usr/bin/env bash
# Issue #8749: real-package smoke for @hono/node-server's module-scope
# `options.createServer || createServerHTTP` binding under compilePackages.

set -uo pipefail
cd "$(dirname "$0")"
. "$(dirname "$0")/../_fixture_lib.sh"

fixture_setup "hono-node-server" || exit 1
fixture_compile_run_diff "hono-node-server"
