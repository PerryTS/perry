### Fixed

- GC: the young leaf trigger compares Eden plus young leaves against Eden's allocation window instead of also counting the survivor space, so a leaf service no longer repeats without retiring anything (Supabase CLI `--version`: 4 futile leaf services gone, -192 M instructions, -2.0%).
