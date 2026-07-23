//! Real V8-`.heapsnapshot` builder over Perry's GC heap walk (#4916).
//!
//! `v8.getHeapSnapshot()` / `v8.writeHeapSnapshot()` used to emit an
//! empty-but-valid V8 heap-snapshot document — Chrome DevTools would
//! load it and show nothing. This module walks the same object
//! population the collector itself sees (every arena + malloc GC
//! allocation on the calling thread) and emits the actual object graph
//! in V8 snapshot JSON: real node types, real `self_size` (GcHeader
//! included), and real reference edges discovered through the same
//! `visit_gc_rewrite_slots` enumeration the marker traces.
//!
//! Approximations (also stated in the public docs):
//! - A full collection runs first, so the dump approximates the live
//!   set. Dead objects in partially-live arena blocks that the sweep
//!   has not reclaimed yet can still appear; free-list slots are
//!   excluded.
//! - Perry arenas are per-thread; the snapshot covers the calling
//!   thread's heap only.
//! - Object inline fields are named through the keys array and arrays
//!   get element indices; closure captures, Map/Set side tables, and
//!   overflow fields appear as `internal` edges with slot ordinals.
//!
//! IMPORTANT: nothing in this module may allocate on the JS heap while
//! the graph is being read — a JS allocation can trigger GC, and a GC
//! can reset or evacuate the very blocks the collected raw pointers
//! point into. Everything below works on Rust-owned buffers.

use super::*;
use std::collections::HashMap;

// Node v26 / V8 13.x layout: ["type","name","id","self_size",
// "edge_count","detachedness"] — `trace_node_id` was dropped upstream.
const NODE_FIELD_COUNT: u32 = 6;

// Indices into the meta `node_types` table emitted below — keep both in sync.
const NODE_TYPE_ARRAY: u32 = 1;
const NODE_TYPE_STRING: u32 = 2;
const NODE_TYPE_OBJECT: u32 = 3;
const NODE_TYPE_CLOSURE: u32 = 5;
const NODE_TYPE_NATIVE: u32 = 8;
const NODE_TYPE_SYNTHETIC: u32 = 9;
const NODE_TYPE_BIGINT: u32 = 13;

// Indices into the meta `edge_types` table.
const EDGE_TYPE_ELEMENT: u32 = 1;
const EDGE_TYPE_PROPERTY: u32 = 2;
const EDGE_TYPE_INTERNAL: u32 = 3;
const EDGE_TYPE_SHORTCUT: u32 = 5;
const EDGE_TYPE_WEAK: u32 = 6;

/// Interned snapshot string table; index 0 is always the empty string.
struct StringTable {
    list: Vec<String>,
    index: HashMap<String, u32>,
}

impl StringTable {
    fn new() -> Self {
        let mut t = StringTable {
            list: Vec::new(),
            index: HashMap::new(),
        };
        t.intern("");
        t
    }

    fn intern(&mut self, s: &str) -> u32 {
        if let Some(&i) = self.index.get(s) {
            return i;
        }
        let i = self.list.len() as u32;
        self.list.push(s.to_string());
        self.index.insert(s.to_string(), i);
        i
    }
}

/// `name_or_index` of an edge: property/shortcut names are string-table
/// indices, element/internal ordinals are plain numbers. Both serialize
/// as integers; the edge type tells consumers which table to look in.
#[derive(Clone, Copy)]
struct Edge {
    edge_type: u32,
    name_or_index: u32,
    to: u32,
}

struct NodeRec {
    header: *mut GcHeader,
    user: usize,
    obj_type: u8,
    self_size: u32,
}

fn json_escape_into(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
}

/// Decode a heap slot the same way `mark_field_into_worklist` does:
/// NaN-boxed pointer tags or a plausible raw 48-bit pointer. Returns 0
/// for anything that cannot reference a heap object.
fn decode_slot_target(bits: u64) -> usize {
    use crate::value::{BIGINT_TAG, POINTER_MASK, POINTER_TAG, STRING_TAG, TAG_MASK};
    let tag = bits & TAG_MASK;
    if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
        return (bits & POINTER_MASK) as usize;
    }
    if (0x1000..=0x0000_FFFF_FFFF_FFFF).contains(&bits) {
        return bits as usize;
    }
    0
}

/// Read a heap string's content without allocating. Accepts STRING_TAG
/// NaN-boxes and raw `StringHeader` pointers; returns `None` for short
/// inline strings and anything that is not a registered heap string.
unsafe fn read_heap_string(bits: u64, max_chars: usize) -> Option<String> {
    let addr = decode_slot_target(bits);
    if addr < GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header = (addr as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader;
    if (*header).obj_type != GC_TYPE_STRING {
        return None;
    }
    let s = addr as *const crate::StringHeader;
    let len = (*s).byte_len as usize;
    if len > (*header).size as usize {
        return None;
    }
    let data = (addr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let raw = String::from_utf8_lossy(std::slice::from_raw_parts(data, len));
    if raw.chars().count() > max_chars {
        let truncated: String = raw.chars().take(max_chars).collect();
        Some(truncated + "…")
    } else {
        Some(raw.into_owned())
    }
}

/// Snapshot node type + display name for one GC object.
unsafe fn node_label(rec: &NodeRec) -> (u32, String) {
    match rec.obj_type {
        GC_TYPE_ARRAY | GC_TYPE_LAZY_ARRAY => (NODE_TYPE_ARRAY, "Array".to_string()),
        GC_TYPE_STRING => {
            let name = read_heap_string(
                crate::value::JSValue::string_ptr(rec.user as *mut crate::StringHeader).bits(),
                128,
            )
            .unwrap_or_default();
            (NODE_TYPE_STRING, name)
        }
        GC_TYPE_OBJECT => {
            let obj = rec.user as *const crate::object::ObjectHeader;
            let class_id = (*obj).class_id;
            let name = if class_id != 0 {
                crate::object::class_name_for_id(class_id).unwrap_or_else(|| "Object".to_string())
            } else {
                "Object".to_string()
            };
            (NODE_TYPE_OBJECT, name)
        }
        GC_TYPE_CLOSURE => {
            let name = {
                let bits = crate::closure::closure_get_dynamic_prop(rec.user, "name").to_bits();
                read_heap_string(bits, 128).unwrap_or_default()
            };
            (
                NODE_TYPE_CLOSURE,
                if name.is_empty() {
                    "()".to_string()
                } else {
                    name
                },
            )
        }
        GC_TYPE_PROMISE => (NODE_TYPE_OBJECT, "Promise".to_string()),
        GC_TYPE_BIGINT => (NODE_TYPE_BIGINT, "bigint".to_string()),
        GC_TYPE_ERROR => (NODE_TYPE_OBJECT, "Error".to_string()),
        GC_TYPE_MAP => (NODE_TYPE_OBJECT, "Map".to_string()),
        GC_TYPE_SET => (NODE_TYPE_OBJECT, "Set".to_string()),
        GC_TYPE_BUFFER => (NODE_TYPE_NATIVE, "Buffer".to_string()),
        GC_TYPE_TYPED_ARRAY => (NODE_TYPE_NATIVE, "TypedArray".to_string()),
        GC_TYPE_DATE_CELL => (NODE_TYPE_OBJECT, "Date".to_string()),
        GC_TYPE_TEMPORAL => (NODE_TYPE_NATIVE, "Temporal".to_string()),
        GC_TYPE_OBJECT_META => (NODE_TYPE_NATIVE, "ObjectMeta".to_string()),
        _ => (NODE_TYPE_NATIVE, "native".to_string()),
    }
}

/// Best-effort property name for inline object field `field_index`,
/// resolved through the object's keys array. `None` for class
/// instances (keys array is NULL) and non-string keys.
unsafe fn object_field_name(
    obj: *const crate::object::ObjectHeader,
    field_index: usize,
) -> Option<String> {
    let keys_bits = (*obj).keys_array as u64;
    let keys_addr = decode_slot_target(keys_bits);
    if keys_addr < GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let keys_header = (keys_addr as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader;
    if (*keys_header).obj_type != GC_TYPE_ARRAY {
        return None;
    }
    let keys = keys_addr as *const crate::array::ArrayHeader;
    if field_index >= (*keys).length as usize {
        return None;
    }
    let elements = (keys_addr + std::mem::size_of::<crate::array::ArrayHeader>()) as *const u64;
    read_heap_string(*elements.add(field_index), 256)
}

/// Build the complete V8-format heap snapshot JSON document for the
/// calling thread's heap. Public entry used by `node_v8.rs`.
pub fn gc_build_v8_heap_snapshot_json() -> String {
    // Approximate the live set: collect first so unreachable malloc
    // objects are freed and fully-dead nursery blocks are reset before
    // the walk picks the population (Node's writeHeapSnapshot also
    // forces a full GC). `js_gc_collect` forces the conservative
    // native-stack scan when no per-thread override is pinned (#4977),
    // so top-level locals held only on the native stack survive.
    js_gc_collect();

    // Free-list slots are dead-but-unreclaimed space inside live
    // blocks; exclude them from the dump.
    let free_slots: std::collections::HashSet<usize> =
        ARENA_FREE_LIST.with(|fl| fl.borrow().iter().map(|&(ptr, _)| ptr as usize).collect());

    let mut recs: Vec<NodeRec> = Vec::new();
    let push_header = |header_ptr: *mut u8, recs: &mut Vec<NodeRec>| unsafe {
        let header = header_ptr as *mut GcHeader;
        let obj_type = (*header).obj_type;
        let size = (*header).size;
        if obj_type == 0 || obj_type > GC_TYPE_MAX || size == 0 {
            return;
        }
        if (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
            return;
        }
        let user = header_ptr as usize + GC_HEADER_SIZE;
        if free_slots.contains(&(header_ptr as usize)) || free_slots.contains(&user) {
            return;
        }
        recs.push(NodeRec {
            header,
            user,
            obj_type,
            self_size: size,
        });
    };

    crate::arena::arena_walk_objects(|header_ptr| push_header(header_ptr, &mut recs));
    let malloc_headers: Vec<*mut GcHeader> = MALLOC_STATE.with(|s| s.borrow().objects.clone());
    for header in malloc_headers {
        push_header(header as *mut u8, &mut recs);
    }

    // Node 0 is the synthetic root; heap objects are nodes 1..=len.
    let mut idx_of: HashMap<usize, u32> = HashMap::with_capacity(recs.len());
    for (i, rec) in recs.iter().enumerate() {
        idx_of.insert(rec.user, (i + 1) as u32);
    }

    let mut strings = StringTable::new();
    let root_name = strings.intern("(GC roots)");

    // Per-node labels, computed before the edge pass so the string
    // table fills in node order.
    let labels: Vec<(u32, u32)> = recs
        .iter()
        .map(|rec| {
            let (node_type, name) = unsafe { node_label(rec) };
            (node_type, strings.intern(&name))
        })
        .collect();

    // Edge pass: enumerate every reference slot the GC itself would
    // trace, keep the ones that point at walked nodes.
    let node_count = recs.len() + 1;
    let mut edges: Vec<Vec<Edge>> = vec![Vec::new(); node_count];
    for (i, rec) in recs.iter().enumerate() {
        let from = i + 1;
        let mut ordinal: u32 = 0;
        let (fields_base, fields_len) = if rec.obj_type == GC_TYPE_OBJECT {
            let obj = rec.user as *const crate::object::ObjectHeader;
            let fc = unsafe { (*obj).field_count } as usize;
            if fc <= 10_000 {
                (
                    rec.user + std::mem::size_of::<crate::object::ObjectHeader>(),
                    fc * 8,
                )
            } else {
                (0, 0)
            }
        } else {
            (0, 0)
        };
        // Element-index naming only for regular arrays — GC_TYPE_LAZY_ARRAY
        // has a different header layout, so its slots fall through to the
        // `internal` label below.
        let (elems_base, elems_len) = if rec.obj_type == GC_TYPE_ARRAY {
            let arr = rec.user as *const crate::array::ArrayHeader;
            (
                rec.user + std::mem::size_of::<crate::array::ArrayHeader>(),
                unsafe { (*arr).length } as usize * 8,
            )
        } else {
            (0, 0)
        };
        unsafe {
            visit_gc_rewrite_slots(rec.header, |slot| {
                let slot_ordinal = ordinal;
                ordinal += 1;
                let bits = *slot.slot;
                let target = decode_slot_target(bits);
                if target == 0 {
                    return;
                }
                let Some(&to) = idx_of.get(&target) else {
                    return;
                };
                if crate::weakref::is_weak_target_trace_slot(rec.header, slot.slot) {
                    edges[from].push(Edge {
                        edge_type: EDGE_TYPE_WEAK,
                        name_or_index: slot_ordinal,
                        to,
                    });
                    return;
                }
                let slot_addr = slot.slot as usize;
                if fields_len > 0
                    && slot_addr >= fields_base
                    && slot_addr < fields_base + fields_len
                {
                    let field_index = (slot_addr - fields_base) / 8;
                    if let Some(name) = object_field_name(
                        rec.user as *const crate::object::ObjectHeader,
                        field_index,
                    ) {
                        edges[from].push(Edge {
                            edge_type: EDGE_TYPE_PROPERTY,
                            name_or_index: strings.intern(&name),
                            to,
                        });
                        return;
                    }
                }
                if elems_len > 0 && slot_addr >= elems_base && slot_addr < elems_base + elems_len {
                    edges[from].push(Edge {
                        edge_type: EDGE_TYPE_ELEMENT,
                        name_or_index: ((slot_addr - elems_base) / 8) as u32,
                        to,
                    });
                    return;
                }
                edges[from].push(Edge {
                    edge_type: EDGE_TYPE_INTERNAL,
                    name_or_index: slot_ordinal,
                    to,
                });
            });
        }
    }

    // Root edges: point the synthetic root at every node with no
    // incoming edge, then sweep up anything still unreachable (pure
    // cycles) so every node is reachable from the root — DevTools
    // drops unreachable nodes.
    let mut indegree = vec![0u32; node_count];
    for per_node in &edges {
        for e in per_node {
            indegree[e.to as usize] += 1;
        }
    }
    let mut reached = vec![false; node_count];
    reached[0] = true;
    let mut queue: Vec<u32> = Vec::new();
    let bfs =
        |start: u32, reached: &mut Vec<bool>, queue: &mut Vec<u32>, edges: &Vec<Vec<Edge>>| {
            queue.push(start);
            while let Some(n) = queue.pop() {
                if reached[n as usize] {
                    continue;
                }
                reached[n as usize] = true;
                for e in &edges[n as usize] {
                    if !reached[e.to as usize] {
                        queue.push(e.to);
                    }
                }
            }
        };
    for i in 1..node_count {
        if indegree[i] == 0 {
            edges[0].push(Edge {
                edge_type: EDGE_TYPE_SHORTCUT,
                name_or_index: root_name,
                to: i as u32,
            });
            bfs(i as u32, &mut reached, &mut queue, &edges);
        }
    }
    for i in 1..node_count {
        if !reached[i] {
            edges[0].push(Edge {
                edge_type: EDGE_TYPE_SHORTCUT,
                name_or_index: root_name,
                to: i as u32,
            });
            bfs(i as u32, &mut reached, &mut queue, &edges);
        }
    }

    let edge_count: usize = edges.iter().map(Vec::len).sum();

    // ---- serialize ----
    let mut out = String::with_capacity(64 * 1024 + node_count * 32 + edge_count * 16);
    out.push_str(concat!(
        r#"{"snapshot":{"meta":{"#,
        r#""node_fields":["type","name","id","self_size","edge_count","detachedness"],"#,
        r#""node_types":[["hidden","array","string","object","code","closure","regexp","number","native","synthetic","concatenated string","sliced string","symbol","bigint","object shape"],"string","number","number","number","number"],"#,
        r#""edge_fields":["type","name_or_index","to_node"],"#,
        r#""edge_types":[["context","element","property","internal","hidden","shortcut","weak"],"string_or_number","node"],"#,
        r#""trace_function_info_fields":["function_id","name","script_name","script_id","line","column"],"#,
        r#""trace_node_fields":["id","function_info_index","count","size","children"],"#,
        r#""sample_fields":["timestamp_us","last_assigned_id"],"#,
        r#""location_fields":["object_index","script_id","line","column"]},"#,
    ));
    out.push_str(&format!(
        r#""node_count":{},"edge_count":{},"trace_function_count":0,"extra_native_bytes":0}},"#,
        node_count, edge_count
    ));

    out.push_str(r#""nodes":["#);
    // Synthetic root: type synthetic, name "(GC roots)", id 1, size 0.
    out.push_str(&format!(
        "{},{},1,0,{},0",
        NODE_TYPE_SYNTHETIC,
        root_name,
        edges[0].len()
    ));
    for (i, rec) in recs.iter().enumerate() {
        let (node_type, name_idx) = labels[i];
        let id = (i + 1) * 2 + 1;
        out.push_str(&format!(
            ",{},{},{},{},{},0",
            node_type,
            name_idx,
            id,
            rec.self_size,
            edges[i + 1].len()
        ));
    }
    out.push_str("],");

    out.push_str(r#""edges":["#);
    let mut first = true;
    for per_node in &edges {
        for e in per_node {
            if !first {
                out.push(',');
            }
            first = false;
            out.push_str(&format!(
                "{},{},{}",
                e.edge_type,
                e.name_or_index,
                e.to * NODE_FIELD_COUNT
            ));
        }
    }
    out.push_str("],");

    out.push_str(
        r#""trace_function_infos":[],"trace_tree":[],"samples":[],"locations":[],"strings":["#,
    );
    for (i, s) in strings.list.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('"');
        json_escape_into(s, &mut out);
        out.push('"');
    }
    out.push_str("]}");
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn snapshot_has_real_nodes_and_edges() {
        // Allocate a recognizable object graph, then snapshot.
        unsafe {
            let marker = b"__heap_snapshot_test_marker__";
            let key = crate::string::js_string_from_bytes(marker.as_ptr(), marker.len() as u32);
            let obj = crate::object::js_object_alloc(0, 1);
            let arr = crate::array::js_array_alloc(2);
            let arr_value = f64::from_bits(crate::value::JSValue::pointer(arr as *const u8).bits());
            crate::object::js_object_set_field_by_name(obj, key, arr_value);
        }
        let json = super::gc_build_v8_heap_snapshot_json();
        assert!(json.starts_with(r#"{"snapshot":{"meta":"#));
        assert!(json.contains(r#""node_count":"#));
        // Real graph: more than just the synthetic root.
        let node_count: usize = json
            .split(r#""node_count":"#)
            .nth(1)
            .and_then(|s| s.split(',').next())
            .and_then(|s| s.parse().ok())
            .expect("node_count parses");
        assert!(node_count > 1, "expected real nodes, got {node_count}");
        assert!(
            json.contains("__heap_snapshot_test_marker__"),
            "string table should contain heap string content"
        );
        // Edges array must be non-empty (root edges at minimum).
        assert!(!json.contains(r#""edges":[]"#));
    }
}
