#!/bin/bash
# Comprehensive test for regex `d` flag (hasIndices) implementation
set -e

PERRY="./target/release/perry"
TEST_DIR="/tmp/perry_regex_d_test"
mkdir -p "$TEST_DIR"

# Test 1: Basic indices
cat > "$TEST_DIR/test1.ts" << 'EOF'
const re = /hello/d;
const m = re.exec("say hello world");
console.log("Test 1: Basic indices");
console.log("  indices:", JSON.stringify(m.indices));
console.log("  Expected: [[4,9]]");
console.log("  Pass:", JSON.stringify(m.indices) === "[[4,9]]");
EOF

# Test 2: Capture groups
cat > "$TEST_DIR/test2.ts" << 'EOF'
const re = /(\w+)@(\w+)/d;
const m = re.exec("email: test@example");
console.log("Test 2: Capture groups");
console.log("  indices:", JSON.stringify(m.indices));
console.log("  Expected: [[7,19], [7,11], [12,19]]");
EOF

# Test 3: Named groups
cat > "$TEST_DIR/test3.ts" << 'EOF'
const re = /(?<year>\d{4})-(?<month>\d{2})/d;
const m = re.exec("date: 2024-03");
console.log("Test 3: Named groups");
console.log("  indices:", JSON.stringify(m.indices));
console.log("  indices.groups:", JSON.stringify(m.indices.groups));
console.log("  Expected groups: {year:[6,10], month:[11,13]}");
EOF

# Test 4: Unmatched optional group
cat > "$TEST_DIR/test4.ts" << 'EOF'
const re = /(\d+)(\.(\d+))?/d;
const m = re.exec("int: 42");
console.log("Test 4: Unmatched optional group");
console.log("  indices:", JSON.stringify(m.indices));
console.log("  indices[2] (unmatched):", m.indices[2]);
console.log("  Expected: undefined");
EOF

# Test 5: Without d flag
cat > "$TEST_DIR/test5.ts" << 'EOF'
const re = /test/;
const m = re.exec("test");
console.log("Test 5: Without d flag");
console.log("  indices:", m.indices);
console.log("  Expected: undefined");
console.log("  hasIndices:", re.hasIndices);
console.log("  Expected: false");
EOF

echo "=== Test 1: Basic indices ==="
$PERRY "$TEST_DIR/test1.ts" -o "$TEST_DIR/test1" 2>&1 | tail -2
"$TEST_DIR/test1"
echo ""

echo "=== Test 2: Capture groups ==="
$PERRY "$TEST_DIR/test2.ts" -o "$TEST_DIR/test2" 2>&1 | tail -2
"$TEST_DIR/test2"
echo ""

echo "=== Test 3: Named groups ==="
$PERRY "$TEST_DIR/test3.ts" -o "$TEST_DIR/test3" 2>&1 | tail -2
"$TEST_DIR/test3"
echo ""

echo "=== Test 4: Unmatched optional group ==="
$PERRY "$TEST_DIR/test4.ts" -o "$TEST_DIR/test4" 2>&1 | tail -2
"$TEST_DIR/test4"
echo ""

echo "=== Test 5: Without d flag ==="
$PERRY "$TEST_DIR/test5.ts" -o "$TEST_DIR/test5" 2>&1 | tail -2
"$TEST_DIR/test5"
echo ""

echo "=== All tests completed! ==="
