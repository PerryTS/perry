Native static layouts reuse canonical key lists from the program image, including class attributes and slot maps. Realm object and function headers now match heap births, and worker evacuation shares immutable lists while copying realm cells.

Fix WASI seed-table linking and first-use registration of runtime prototype layouts.

Avoid hashing default attribute entries when building dynamic canonical key lists.

Keep compiler layout spans ahead of the checked WASI runtime-descriptor enumeration when integrating current main.
