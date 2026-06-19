#!/usr/bin/env bash
# Post-build freshness guard for runtime static libraries (#4856).
#
# A Swatinem/rust-cache restore can leave `target/` fingerprints that make
# cargo treat workspace crates as up-to-date and silently reuse a
# `libperry_runtime.a` built from older sources — v0.5.1150 shipped Apple
# cross runtimes missing `perry_macos_bundle_chdir` exactly that way, which
# broke every iOS/tvOS executable link. This script asserts that a built
# runtime archive defines the sentinel `#[no_mangle]` symbols below, so a
# stale archive fails the release instead of shipping.
#
# Usage: check_runtime_symbols.sh <libperry_runtime.a | perry_runtime.lib>...
#
# When codegen starts referencing a new unconditional runtime symbol from
# every program's `main` prelude (see perry-codegen/src/codegen/entry.rs),
# add it to SENTINELS so a stale runtime missing it is caught here, not on
# a build worker at link time.

set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "usage: $0 <runtime-archive>..." >&2
  exit 2
fi

# Each sentinel must be defined unconditionally in perry-runtime — i.e. a
# `#[no_mangle] pub extern "C" fn` with no `#[cfg]` on the item or its
# module (a cfg-gated *body* is fine; the symbol still exists everywhere).
SENTINELS=(
  js_gc_init
  js_typed_feedback_maybe_dump_trace
  perry_macos_bundle_chdir # added by #4833; absence = pre-#4833 stale archive
  js_array_numeric_value_to_raw_f64
  js_array_mark_numeric_f64_layout
  js_array_clear_numeric_layout
  js_array_note_numeric_write
  js_array_is_numeric_f64_layout
  js_array_numeric_get_f64_unboxed
  js_array_numeric_set_f64_unboxed
  js_array_numeric_push_f64_unboxed
  js_typed_f64_arg_guard
  js_typed_f64_arg_to_raw
  js_typed_i32_arg_guard
  js_typed_i32_arg_to_raw
  js_typed_i1_arg_guard
  js_typed_i1_arg_to_raw
  js_typed_string_arg_guard
  js_typed_string_arg_to_raw
  js_box_alloc_bits
  js_box_get_bits
  js_box_set_bits
  js_closure_get_capture_bits
  js_closure_set_capture_bits
  js_object_get_field_by_property_id_f64
  js_object_set_field_by_property_id
  js_native_call_method_by_id
  js_native_call_method_apply_by_id
  js_class_method_bind_by_id
  js_method_direct_shape_guard
  js_typed_feedback_class_field_get_guard
  js_typed_feedback_class_field_set_guard
  js_typed_feedback_method_direct_call_guard
  js_typed_feedback_closure_direct_call_guard
  js_typed_feedback_array_get_f64
  js_typed_feedback_plain_array_index_get_guard
  js_typed_feedback_numeric_array_index_get_guard
  js_typed_feedback_packed_f64_array_loop_guard
  js_typed_feedback_packed_i32_array_loop_guard
  js_typed_feedback_packed_u32_array_loop_guard
  js_typed_feedback_array_index_get_fallback_boxed
  js_typed_feedback_array_set_f64
  js_typed_feedback_array_set_f64_extend
  js_typed_feedback_plain_array_index_set_guard
  js_typed_feedback_numeric_array_index_set_guard
  js_typed_feedback_numeric_array_push_guard
  js_typed_feedback_array_index_set_fallback_boxed
  js_typed_feedback_observe_array_element
  js_typed_feedback_array_set_string_key
  js_typed_feedback_array_set_index_or_string
  js_typed_feedback_object_set_index_polymorphic
  js_typed_feedback_object_set_unboxed_f64_field
  js_map_set_string_number
  js_map_set_string_key
  js_map_set_string_i32
  js_map_set_string_u32
  js_map_set_string_f32
  js_map_set_string_bool
  js_map_set_string_string
  js_map_set_number_key
  js_map_get_string_key
  js_map_get_number_key
  js_map_has_string_key
  js_map_has_number_key
  js_map_delete_string_key
  js_map_delete_number_key
  js_set_add_string
  js_set_add_number
  js_set_has_string
  js_set_has_number
  js_set_delete_string
  js_set_delete_number
  js_set_add_i32
  js_set_has_i32
  js_set_delete_i32
  js_set_add_u32
  js_set_has_u32
  js_set_delete_u32
  js_set_add_f32
  js_set_has_f32
  js_set_delete_f32
  js_set_add_bool
  js_set_has_bool
  js_set_delete_bool
  js_i32_box_alloc
  js_i32_box_get
  js_i32_box_set
  js_bool_box_alloc
  js_bool_box_get
  js_bool_box_set
  js_iter_result_set
  js_iter_result_set_f64
  js_iter_result_set_i32
  js_iter_result_set_i1
  js_iter_result_get_value
  js_iter_result_get_value_f64
  js_iter_result_get_value_i32
  js_iter_result_get_value_i1
  js_iter_result_get_done
  js_typed_feedback_native_call_method_by_id
  js_typed_feedback_native_call_method_apply_by_id
)

# Tool preference: rustup's llvm-tools nm (matches rustc's LLVM, reads the
# thin-LTO bitcode members) → PATH llvm-nm → system nm. The fallbacks may
# fail to parse bitcode members, but `--print-armap` below only needs the
# archive symbol index (ranlib map), which every archiver writes as plain
# data — readable regardless of member object format.
NM=""
if command -v rustc >/dev/null 2>&1; then
  sysroot=$(rustc --print sysroot 2>/dev/null || true)
  host=$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')
  if [ -n "$sysroot" ] && [ -n "$host" ] && [ -x "$sysroot/lib/rustlib/$host/bin/llvm-nm" ]; then
    NM="$sysroot/lib/rustlib/$host/bin/llvm-nm"
  fi
fi
if [ -z "$NM" ] && command -v llvm-nm >/dev/null 2>&1; then
  NM=llvm-nm
fi
if [ -z "$NM" ] && command -v nm >/dev/null 2>&1; then
  NM=nm
fi
if [ -z "$NM" ]; then
  echo "::warning::check_runtime_symbols: no llvm-nm/nm available — skipping symbol guard" >&2
  exit 0
fi

status=0
for lib in "$@"; do
  if [ ! -f "$lib" ]; then
    echo "::error::check_runtime_symbols: $lib does not exist" >&2
    status=1
    continue
  fi
  # `--print-armap` emits the archive symbol index ("sym in member.o") in
  # addition to per-member listings; unreadable members only lose the latter.
  # Some llvm-nm builds under-report ELF archive indices for symbols kept alive
  # via `#[used]` fn-pointer statics, while GNU nm reports the same archive
  # correctly with `-s`. Merge both views when available, then exact-match — no
  # substring false positives (`foo_js_gc_init` ≠ `js_gc_init`).
  tokens=$(
    {
      "$NM" --print-armap "$lib" 2>/dev/null || true
      "$NM" -g "$lib" 2>/dev/null || true
      if command -v nm >/dev/null 2>&1; then
        nm -s "$lib" 2>/dev/null || true
        nm -g "$lib" 2>/dev/null || true
      fi
    } | tr -d '\r' | tr ' \t' '\n\n' | sed 's/^_//' | sort -u
  )
  missing=0
  for sym in "${SENTINELS[@]}"; do
    if ! grep -qx "$sym" <<<"$tokens"; then
      echo "::error::$lib does not define runtime symbol '$sym' — stale cached build artifact? (#4856)" >&2
      missing=1
      status=1
    fi
  done
  if [ "$missing" -eq 0 ]; then
    echo "ok: $lib defines all ${#SENTINELS[@]} sentinel symbols"
  fi
done
exit "$status"
