#!/bin/bash
# A1 lint: object-header access must go through crates/perry-runtime/src/object/header.rs.
# Usage: scripts/check_object_header_access.sh <dir> [--lines]
#        scripts/check_object_header_access.sh --self-test
#
# Counts (test files excluded), with file:line under --lines:
#   (a) bare GC_TYPE_OBJECT compares, match arms and `_of_type(.., GC_TYPE_OBJECT)` kind tests
#       outside gc/types.rs and object/header.rs
#   (b) size_of::<..ObjectHeader>() and ObjectHeader).add(1) slot-base arithmetic outside object/header.rs
#   (c) direct `.meta` field accesses outside object/header.rs and object/meta_accessors.rs
#   (d) A1_UNPROVEN markers: funnel sites whose owner is not proven meta-form (kind-A/B question)
#   (e) bare GC_TYPE_ARRAY compares, match arms, `Some(..)`, asserts and kind-helper arguments
#       (`*_of_type(..)`, `*_gc_type(..)`, codegen `icmp_*`) outside perry-abi/src/array_type.rs and
#       codegen's target_layout.rs (the emitted-compare twin)
#       (value records VR-4a: every array-type test goes through `gc_type_is_array!`,
#       `gc_type_is_not_array!` or `gc_array_type_pat!`, and codegen's
#       `target_layout::emit_gc_type_is_{,not_}array`). Births (allocation arguments, header
#       writes) still name the constant. Not visible to a line lint: a test whose constant sits
#       alone on a continuation line, and a codegen compare against a bare "1" literal.
# Prints one table per directory (crates/<crate>/src/<top>) and the total.
# Count mode (slice S0a): exits 0. S0f flips it to an error with A1_LINT_MODE=error.
# (e) is always an error: it is at zero, and CI (`lint`) keeps it there.
if [ "$1" = "--self-test" ]; then
  self=$(cd "$(dirname "$0")" && pwd)/$(basename "$0"); t=$(mktemp -d); trap 'rm -rf "$t"' EXIT
  mkdir -p "$t/crates/perry-runtime/src"; f="$t/crates/perry-runtime/src/probe.rs"
  printf 'fn ok(t: u8) -> bool { crate::gc_type_is_array!(t) }\n// t == crate::gc::GC_TYPE_ARRAY\n' > "$f"
  bash "$self" "$t" > /dev/null || { echo "self-test FAILED: a migrated site was rejected"; exit 1; }
  for bad in 'if t == crate::gc::GC_TYPE_ARRAY {}' 'if t != GC_TYPE_ARRAY {}' 'GC_TYPE_ARRAY => 1,' \
             'Some(crate::gc::GC_TYPE_ARRAY | x) => 1,' 'k == Some(crate::gc::GC_TYPE_ARRAY)' \
             'debug_assert_eq!(t, crate::gc::GC_TYPE_ARRAY);' 'heap_ptr_with_gc_type(v, crate::gc::GC_TYPE_ARRAY)' \
             'blk.icmp_eq(I8, &t, GC_TYPE_ARRAY)'; do
    printf '%s\n' "$bad" > "$f"
    if bash "$self" "$t" > /dev/null; then echo "self-test FAILED: not rejected: $bad"; exit 1; fi
  done
  echo "check_object_header_access self-test: ok"; exit 0
fi
dir=${1:?usage: check_object_header_access.sh <dir> [--lines]}; lines=$2
cd "$dir" || exit 2
SRC=$(ls -d crates/perry-runtime/src crates/perry-stdlib/src crates/perry-codegen/src crates/perry/src crates/perry-abi/src crates/perry-runtime-static crates/perry-stdlib-static crates/perry-ext-zlib crates/perry-perex 2>/dev/null)
P='(crate::gc::|gc::|crate::|perry_runtime::gc::|perry_abi::|crate::codegen_abi::|crate::runtime_abi::)?'
find_rs() { grep -rnP "$1" --include=*.rs $SRC 2>/dev/null | grep -vE "/tests?/|_tests?\.rs:|/tests?\.rs:|/test_[a-z_]*\.rs:"; }
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
find_rs "(==|!=)\s*${P}GC_TYPE_OBJECT\b(?!_)|\bGC_TYPE_OBJECT\b(?!_)\s*(=>|\||==|!=)|\|\s*${P}GC_TYPE_OBJECT\b(?!_)|matches!\([^)]*\bGC_TYPE_OBJECT\b(?!_)|_of_type\([^)]*\bGC_TYPE_OBJECT\b(?!_)" \
  | grep -vE "^crates/perry-runtime/src/(gc/types|object/header)\.rs:|^crates/perry-abi/src/object_type\.rs:" | grep -vP "^\S+:\d+:\s*//" > "$tmp/a"
find_rs "size_of::<(crate::object::|super::|crate::|perry_runtime::)?ObjectHeader>\(\)|ObjectHeader>\(\)\.add|as \*(mut|const) ObjectHeader\)\.add\(1\)|ObjectHeader\)\.add\(1\)" \
  | grep -vE "^crates/perry-runtime/src/object/header\.rs:" | grep -vP "^\S+:\d+:\s*//" > "$tmp/b"
find_rs "\.meta\b(?!\()" | grep -E "^crates/(perry-runtime|perry-stdlib|perry-codegen|perry-ext-zlib|perry-perex)/" | grep -vE "import\.meta|shadow_stack|\(\*entry\)\.meta|module\.meta|\.meta\.(span|kind)" \
  | grep -vE "^crates/perry-runtime/src/object/(header|meta_accessors|cell_meta)\.rs:" | grep -vP "^\S+:\d+:\s*//" > "$tmp/c"
find_rs "A1_UNPROVEN" > "$tmp/d"
Q='(\w+::)*GC_TYPE_ARRAY\b(?!_)'
find_rs "(==|!=)\s*$Q|\bGC_TYPE_ARRAY\b(?!_)\s*(=>|\||==|!=)|\|\s*$Q|matches!\([^)]*\bGC_TYPE_ARRAY\b(?!_)|Some\(\s*$Q|assert(_eq|_ne)?!\([^;]*\bGC_TYPE_ARRAY\b(?!_)|(_of_type|_gc_type|gc_type_child|icmp_eq|icmp_ne)\([^)]*\bGC_TYPE_ARRAY\b(?!_)" \
  | grep -vE "^crates/perry-abi/src/array_type\.rs:|^crates/perry-codegen/src/target_layout\.rs:" | grep -vP "^\S+:\d+:\s*//|^\S+:\d+:const _: \(\) = assert!\(GC_TYPE_ARRAY == perry_abi::GC_TYPE_ARRAY\);" > "$tmp/e"
key() { awk -F: '{n=split($1,p,"/"); if (p[1]=="crates" && p[3]=="src" && n>4) print p[1]"/"p[2]"/src/"p[4]; else if (p[1]=="crates" && n>=3) print p[1]"/"p[2]"/src (top)"; else print $1}'; }
for k in a b c d e; do key < "$tmp/$k" | sort | uniq -c | awk -v k=$k '{c=$1; $1=""; sub(/^ /,""); print $0"\t"k"\t"c}'; done > "$tmp/counts"
printf "%-52s %8s %8s %8s %8s %8s %8s\n" directory "(a)type" "(b)base" "(c)meta" "(d)unprv" "(e)array" total
awk -F'\t' '{d[$1]; v[$1,$2]=$3} END {for (x in d) printf "%s\t%d\t%d\t%d\t%d\t%d\n", x, v[x,"a"], v[x,"b"], v[x,"c"], v[x,"d"], v[x,"e"]}' "$tmp/counts" | sort \
  | awk -F'\t' '{t=$2+$3+$4+$5+$6; printf "%-52s %8d %8d %8d %8d %8d %8d\n", $1,$2,$3,$4,$5,$6,t; A+=$2;B+=$3;C+=$4;D+=$5;E+=$6} END {printf "%-52s %8d %8d %8d %8d %8d %8d\n", "TOTAL",A,B,C,D,E,A+B+C+D+E}'
if [ "$lines" = "--lines" ]; then for k in a b c d e; do echo "== ($k)"; cut -d: -f1,2 "$tmp/$k"; done; fi
if [ -s "$tmp/e" ]; then echo "(e) raw GC_TYPE_ARRAY tests: use gc_type_is_array! / gc_type_is_not_array! / gc_array_type_pat! (perry-abi/src/array_type.rs)"; cat "$tmp/e"; exit 1; fi
if [ "$A1_LINT_MODE" = error ] && [ -s <(cat "$tmp/a" "$tmp/b" "$tmp/c" "$tmp/d") ]; then exit 1; fi
exit 0
