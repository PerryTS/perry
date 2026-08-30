### Fixed

- Object-property tombstone deletes are opt-in again while an evacuating-GC interaction with class dispatch is repaired, preventing deleted class instances from losing their keys or returning incorrect field values after collection.
