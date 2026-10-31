### Fixed

- Share MessagePort core operations between prototypes and instances, honoring borrowed-method receivers and rejecting invalid receivers before message cloning. Keep port identity in existing intrinsic private storage rather than a mutable public field.
