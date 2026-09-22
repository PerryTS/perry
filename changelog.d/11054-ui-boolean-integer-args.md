### Fixed

- Perry UI integer arguments now convert JavaScript booleans before entering native ABIs, so `widgetSetHidden(widget, false)` reliably shows a previously hidden widget. (#11048)
