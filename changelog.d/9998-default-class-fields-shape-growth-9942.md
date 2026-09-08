**Default class-field definitions no longer retain a fresh shape descriptor
for every constructed instance** (#9942). `CreateDataProperty` supplies the
ordinary writable, enumerable and configurable attributes, whose absence from
Perry's descriptor side table already means the same thing. Recording those
defaults anyway minted a process-unique semantic shape generation on every
Drizzle `MySqlSelectBuilder.from` construction; one shared keys family grew to
hundreds of thousands of uncarried descriptors during an idle scheduler loop.
The define path now omits that redundant descriptor entry and semantic
transition, while a real customized-to-default attribute change still clears
the old entry and invalidates its prior shape.
