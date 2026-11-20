Read Mach-O addon import names from the declared undefined-symbol range without treating dynamic-lookup or executable ordinals as dylib indexes. This permits valid Node-API addons on macOS while retaining unsupported libuv/V8/Node C++ import checks, including prebound undefined symbols. The host integration test uses the same import reader and normalizes Mach-O symbol decoration.

Preserve registered `@parcel/watcher` facade imports when CommonJS wrapping resolves explicit or wildcard `compilePackages` entries. Resolving those imports to the underlying `.node` path bypassed the facade and produced a non-module value. The JavaScript wrapper subpath continues compiling from source.

Retain exported Node-API symbols during final executable stripping, as for plugin hosts, so macOS addons can actually resolve the host ABI. Make the watcher differential fixture use canonical paths and the brute-force snapshot backend; assert the five expected events before comparing Perry, avoiding both delayed FSEvents and a vacuous empty-stream pass.

Keep the 600 KiB host budget against a CommonJS control, matching the addon application instead of charging Node-API for CommonJS initialization. Recompile the unused-policy control at the same output path, so the zero-byte assertion is not affected by the executable basename embedded in Mach-O signing metadata.
