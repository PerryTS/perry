Emit executable pool literals as complete immutable string headers and pre-initialized NaN-boxed handles in the code image. Short equal literals coalesce by their full byte spelling at link time. Literal startup allocation, per-literal initialization, global-root registration and arena residency disappear. Static dispatch descriptors and literal handles reference the same immutable interned image header, keeping identity stable without weak-cache rematerialization.

Delete `ATOM_YOUNG`, `INTERN_YOUNG`, atom/intern scanners, shims and registration. Dynamic interning remains weak under the existing heap-change invalidation contract, with the existing 8192-slot reuse capacity retained. A 1024-slot experiment caused repeat transition misses on Qs; exact short-key transition identity recovered part of the loss, but still missed the performance gate. The smaller residual atom index serves runtime-created static key lists, intrinsic site helpers and unloadable output; complete removal needs those producers to carry their own image headers/descriptors. Unloadable output retains owned literals so escaped strings survive unloading.

Image headers are read-only, born marked as well as pinned, and shared across workers. Precise full-GC roots can visit an image leaf without writing its header. Redundant refcount/provenance writes are skipped, and noncanonical WTF-8 images retain their absent validation proof. Header constants are checked against runtime declarations. Root-holder entries for deleted tables disappear, with the retained weak contract visible and source-pinned. The full-cycle census snapshot contract is re-audited after scanner removal.

Image keys publish their original identity into existing keyed sites and use the existing interned-key eligibility checks. No new write-proof predicates remain. Native-IR checks also exposed two scalar-rooting mistakes on main: callback-rebindable module cells and member updates that can return BigInt now retain precise custody.

When an existing shape-slot guess confirms an equal heap key, its canonical key owner retains the immutable image spelling. Shared prefixes keep the same string values, positions and pointer masks; later reads compare identity without repeating whole-key-list scans. This uses one shared runtime helper and the existing array store barrier, with no additional site state.

Builtin layout births probe the existing canonical owner directly with borrowed key bytes, instead of requiring a weak-intern-cache hit before recognizing an already-live layout. This removes repeated temporary key construction after weak-cache invalidation.

Weak entries are never published inside a heap-change scope. Collection-entry and string-realloc invalidation therefore keep stale addresses out of nested changes, removing per-lookup scope checks and repeated clearing without adding state.

Runtime builtin-global reads borrow the spelling from the existing global shape before their allocating fallback. Frequent String, Boolean, Promise and Number reads keep the owner-held key identity through collections, with no added table or site state.

Mixed inline/heap string equality uses one shared borrowed-byte helper instead of materializing weak interns through two unified-pointer calls. The existing non-string fallback retains its coercion behavior with precise custody across allocations; the emitted call sequence shrinks.

Raw uses of proven image literals now consume their image address directly, removing a boxed relocation, pointer mask and any unified-string conversion without adding emitted guards or runtime state.

Weak reuse survives unrelated array/object reallocations. Collection entry still clears every slot, and tracked string reallocations clear before memory can move; publication remains forbidden throughout all heap-change scopes. Removing redundant scope-close clears avoids repeated weak rematerialization without retaining dead strings or adding state.

Restore the missing ordinary integrity-flags argument at spread-result shape birth after rebasing onto the shaped-spread optimization.

Boxed image consumers retain one read of the preinitialized immutable handle; raw-pointer consumers fold directly to their image address before LLVM. This removes repeated tag arithmetic while preserving typed-property literal folding and adds no runtime state or startup work.

Computed-read sites reuse the equal string already owned and traced in their existing four-way record when dynamic input headers are rematerialized. Rebuilding a weak intern no longer consumes extra ways or saturates the site; receiver/holder mutation checks stay in the existing shared proof reader.

Raw image-address folding emits ordinary SSA GEPs supported by both native codegen-unit construction and textual LLVM, with a native-reader verifier check on the actual folded fixture.

Content-confirmed own-data positions retain their image key in the same canonical owner as positional reads. Literal-tail overwrites after a spread then use the existing identity proof instead of repeatedly taking the content fallback; field values, attributes and ShapeId remain unchanged.
