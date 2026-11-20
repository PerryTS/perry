Deleting a property from an object whose Number-only fields are stored
unboxed no longer runs an extra release step first; the delete already moves
the object to a general shape before it shifts any value.
