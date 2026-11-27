Aliased better-sqlite3 constructors now open the database correctly. Constructor arguments are evaluated in order, and a throwing argument prevents the database from being opened.
