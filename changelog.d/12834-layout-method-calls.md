Unknown receiver method calls can use an immutable method index on their class family layout. Each entry names the complete static prototype holder proof and method slot; current holder words and the receiver layout prove the answer before a direct native body call. Own shadows, prototype mutations, accessors, Proxy receivers and other proof misses retain ordinary lookup and dispatch. Split lookup/call sites and by-id calls share the index without site state or a cache.

Fresh instance-method entries whose bodies cannot consume a lexical-home scope omit the unused enter/leave work while retaining their function objects’ home captures. Methods using private names, environments, super, nested closures or constructor references keep the scope.

Known-class calls with observable arguments now read a mutable method before evaluating those arguments, through the existing split lookup/call path. This preserves the selected function when an argument replaces it.
