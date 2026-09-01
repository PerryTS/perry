The `warnings` gate is green again. Five test modules in `perry-runtime` still
imported `std::os::raw::c_int` after the code that used it was removed, which
fails a `-D warnings` build.
