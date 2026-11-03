A class with private methods or accessors installs its private brand on a new
instance before its fields are defined. The branded shape is now recognised as
the class family layout it came from (a brand changes no slot), so field
definitions fill their layout slots. Before, they were appended after the
empty slots, and a read at the slot the layout names returned `undefined`.
