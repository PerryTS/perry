**perf(typedarray): a typed array's own header says where its elements live (#10516)**

One live `ArrayBuffer`-aliasing view anywhere in the process (noble's
`u32(arr)`, an `isLE` probe that is garbage at once) sent EVERY typed-array
element access in the program to the out-of-line slow path, because the
inline element paths were licensed by a process-wide count of live views
(`PERRY_TA_VIEW_GUARD`). The sha256 message-schedule fixture went from about
30,900 to 109,900 instructions per block (53x node) the moment one view
existed, and stayed there after the view died.

`TypedArrayHeader` now carries a storage byte (header byte 10, formerly
padding): `TA_STORAGE_INLINE` for an owning array, `TA_STORAGE_EXTERNAL` once
it aliases an `ArrayBuffer` (constructed as a view, or given a `.buffer`) and
for native-arena views. The kind-cache tag carries the same fact
(`kind | 0x80` for external storage), so every emitted guard that compares
the tag with the kind it expects rejects a view by that same compare, and the
two guards that read the header (`.length`, the dynamic element read) test the
byte. `data_ptr` answers an inline array from the byte without probing the
view tables. The global counter is gone.

liveview / deadview: 109,871 -> 30,071 instructions per block, equal to
noview (30,891 -> 30,071). What remains is ToInt32 of the element reads
(`js_number_coerce`), which belongs to the bitwise lane (#10511).

Test: `test-files/test_gap_10516_typed_view_storage.ts` (owning arrays read
inline before and while views live, views and `subarray` aliasing in both
directions, an owning array that gains a `.buffer` after inline reads,
out-of-bounds reads).
