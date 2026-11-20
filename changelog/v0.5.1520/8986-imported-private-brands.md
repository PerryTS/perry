Imported private brands are installed once, by the defining module's standalone constructor.

A metadata-only imported class stub is now identified explicitly, so importing a class that uses private elements no longer re-runs brand installation in the importing module. Re-branding produced a second brand for the same class, so a private access that had been valid through one import path failed through the other.

Covers direct imports, accessors, local and imported subclasses, same-module branding, and genuine duplicate initialization.
