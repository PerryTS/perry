#!/bin/bash
# Test for regex `d` flag (hasIndices) implementation
set -e

PERRY="./target/release/perry"
TEST_DIR="/tmp/perry_regex_d_test"
mkdir -p "$TEST_DIR"

# Simple test
cat > "$TEST_DIR/test.ts" << 'EOF'
const re = /hello/d;
const m = re.exec("say hello world");
console.log("indices:", JSON.stringify(m.indices));
console.log("hasIndices:", re.hasIndices);
EOF

echo "Compiling test..."
$PERRY "$TEST_DIR/test.ts" -o "$TEST_DIR/test" 2>&1 | tail -5

echo ""
echo "Running test..."
"$TEST_DIR/test"

echo ""
echo "Test completed!"
