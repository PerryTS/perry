A static-key store site emits the class-setter arm (#10498) only for a property
name that some compiled class of the program declares as a setter. The runtime
admits an entry only for a declared accessor, so every other store carried an
arm it could never take and ran it on every miss. The driver collects the
names over all modules and passes them to codegen; they are part of the object
cache key. Key-add cells lose 13 instructions per operation, and the compiled
tsc workload drops 1.1 MB.
