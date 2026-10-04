A static-key read site emits the class-getter arm (#10498) only for a property
name that some compiled class of the program declares as a getter, as the store
site already does for setters. The runtime admits an entry only for a declared
accessor, so every other read carried an arm it could never take. The compiled
tsc workload drops 7.8 MB (181.5 MB to 173.7 MB).
