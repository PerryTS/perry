#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

echo "=== Perry UI runtime regression tests ==="

node "$ROOT/tests/ui/web_keyboard_polling_installs_listeners.cjs"
echo "PASS ui/web_keyboard_polling_installs_listeners"
