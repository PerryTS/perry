Fixed AOT module collection so statically reachable JavaScript and CommonJS
dependencies allowed by the host trust policy compile natively without each
package also needing a `perry.compilePackages` routing entry.
