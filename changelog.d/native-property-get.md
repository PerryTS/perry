Speed up native property Get for ordinary own and prototype data properties.
The shared lookup borrows keys and checks shape and accessor metadata before
returning, while getters, proxies, class statics, and exotics retain their
existing dispatch. JSON.stringify reuses a canonical toJSON key.

Linux user-instruction counts decreased 71–78% in Reflect.get probes, 12% in
positive thenable assimilation, and 43% in inherited toJSON serialization; the
user-iterator control was unchanged. Differential tests, Node parity, fault
injections, and measurement/validation receipts are recorded in
benchmarks/native_property_get/REPORT.md.
