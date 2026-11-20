Fixed an intermittent moving-GC failure while reading callable properties from
short-named native-module namespaces such as `net`. Module names now remain
owned and namespace receivers stay rooted while bound exports are resolved.
