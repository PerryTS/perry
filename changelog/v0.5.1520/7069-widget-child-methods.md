Fixed `Widget.addChild()` and `Widget.removeAllChildren()` on native targets.
Widget handles returned by ordinary `perry/ui` factories, as well as parameters
typed as `Widget`, now route these compatibility methods through the same native
FFI operations as `widgetAddChild()` and `widgetClearChildren()` instead of
falling through to a silent dynamic no-op.
