**Dynamic method calls no longer probe every native-kind side registry for arena values.**
Ordinary objects and other GC-managed receivers are classified from their arena metadata and
resident header, avoiding the armed Symbol registry's global lock and address hash. Persistent
boxed symbols and process-global shared buffers retain the existing safe registry fallback.
