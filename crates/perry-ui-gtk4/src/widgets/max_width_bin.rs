//! `MaxWidthBin` — a one-child GtkWidget that reproduces CSS `max-width`.
//!
//! GTK4 has no max-width: its CSS honours only `min-width`, `set_size_request`
//! sets a minimum, and `halign`/`hexpand` give either a natural-size or a
//! full-fill child, never "fill up to a cap then stop". So `widgetSetMaxWidth`
//! wraps the child in this bin, which measures and allocates the child itself:
//! below the cap the child fills the bin's width, at and above it the child is
//! held at `max_width` and centered. The bin `hexpand`s, so it spans the parent
//! and the gutters land inside it.
//!
//! The measure/allocate logic is verified against real GTK4: at a 900px width
//! the child allocates 320px centered (x=290); at 200px it fills (200px, x=0).

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;

mod imp {
    use gtk4::glib;
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    pub struct MaxWidthBin {
        pub child: RefCell<Option<gtk4::Widget>>,
        pub max: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MaxWidthBin {
        const NAME: &'static str = "PerryMaxWidthBin";
        type Type = super::MaxWidthBin;
        type ParentType = gtk4::Widget;
    }

    impl ObjectImpl for MaxWidthBin {
        fn dispose(&self) {
            if let Some(c) = self.child.borrow_mut().take() {
                c.unparent();
            }
        }
    }

    impl WidgetImpl for MaxWidthBin {
        fn measure(&self, orientation: gtk4::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some(c) = self.child.borrow().as_ref().cloned() else {
                return (0, 0, -1, -1);
            };
            let (min, nat, min_base, nat_base) = c.measure(orientation, for_size);
            if orientation == gtk4::Orientation::Horizontal {
                let cap = self.max.get();
                // A small min lets the bin shrink to fill a narrow parent; a
                // natural capped at the cap never requests more than max_width.
                (min.min(cap), nat.min(cap), min_base, nat_base)
            } else {
                (min, nat, min_base, nat_base)
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            let Some(c) = self.child.borrow().as_ref().cloned() else {
                return;
            };
            let w = width.min(self.max.get());
            let x = (width - w).max(0) / 2;
            c.size_allocate(&gtk4::Allocation::new(x, 0, w, height), baseline);
        }
    }
}

glib::wrapper! {
    pub struct MaxWidthBin(ObjectSubclass<imp::MaxWidthBin>) @extends gtk4::Widget;
}

impl MaxWidthBin {
    /// Wrap `child` (which must be unparented) and cap its width at `max`.
    pub fn wrap(child: &impl IsA<gtk4::Widget>, max: i32) -> Self {
        let bin: Self = glib::Object::new();
        child.set_parent(&bin);
        bin.imp().child.replace(Some(child.clone().upcast()));
        bin.imp().max.set(max);
        bin.set_hexpand(true);
        bin
    }

    /// Update the cap on an existing bin (idempotent re-apply).
    pub fn set_max(&self, max: i32) {
        self.imp().max.set(max);
        self.queue_resize();
    }
}
