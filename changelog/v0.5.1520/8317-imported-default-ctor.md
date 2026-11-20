Fixed construction of local subclasses whose imported parent inherits a default
constructor. Perry now invokes the runtime parent once with the original arguments
and preserves fields initialized by the imported constructor, fixing Drizzle MySQL
column creation failures such as writes to `config.uniqueName`.
