use objc2_app_kit::NSView;
use objc2_core_foundation::CGSize;
use objc2_foundation::MainThreadMarker;
use perry_ui_macos::{app, widgets};

pub fn check(mtm: MainThreadMarker) {
    for before in [true, false] {
        for scroll in [true, false] {
            let host = NSView::new(mtm);
            host.setFrameSize(CGSize::new(1200.0, 400.0));
            let parent = if scroll {
                widgets::scrollview::create()
            } else {
                widgets::zstack::create()
            };
            let parent_view = widgets::get_widget(parent).unwrap();
            host.addSubview(&parent_view);
            parent_view.setTranslatesAutoresizingMaskIntoConstraints(false);
            let width = parent_view.widthAnchor().constraintEqualToConstant(1000.0);
            width.setActive(true);
            parent_view
                .heightAnchor()
                .constraintEqualToConstant(300.0)
                .setActive(true);
            let column = widgets::vstack::create(0.0);
            if before {
                widgets::set_max_width(column, 640.0);
            }
            if scroll {
                widgets::scrollview::set_child(parent, column);
            } else {
                widgets::add_child(parent, column);
            }
            host.layoutSubtreeIfNeeded();
            if !before {
                widgets::set_max_width(column, 640.0);
            }
            for size in [1000.0, 400.0, 1000.0] {
                width.setConstant(size);
                host.layoutSubtreeIfNeeded();
                let column_view = widgets::get_widget(column).unwrap();
                let rect = column_view.convertRect_toView(column_view.bounds(), Some(&parent_view));
                assert!(
                    (rect.size.width - size.min(640.0)).abs() < 1.0,
                    "scroll={scroll}, before={before}: {rect:?}"
                );
                assert!(
                    (rect.origin.x - (size - size.min(640.0)) / 2.0).abs() < 1.0,
                    "scroll={scroll}, before={before}: {rect:?}"
                );
            }
        }
        let app_handle = app::app_create(std::ptr::null(), 1000.0, 400.0);
        let root = widgets::vstack::create(0.0);
        if before {
            widgets::set_max_width(root, 640.0);
        }
        app::app_set_body(app_handle, root);
        let root_view = widgets::get_widget(root).unwrap();
        let window = root_view.window().unwrap();
        window.contentView().unwrap().layoutSubtreeIfNeeded();
        if !before {
            widgets::set_max_width(root, 640.0);
        }
        for width in [1000.0, 400.0, 1000.0] {
            window.setContentSize(CGSize::new(width, 400.0));
            let content = window.contentView().unwrap();
            content.layoutSubtreeIfNeeded();
            let rect = root_view.convertRect_toView(root_view.bounds(), Some(&content));
            assert!(
                (rect.size.width - width.min(640.0)).abs() < 1.0,
                "root before={before}: {rect:?}"
            );
            assert!(
                (rect.origin.x - (width - width.min(640.0)) / 2.0).abs() < 1.0,
                "root before={before}: {rect:?}"
            );
        }
    }
}
