Added `App({ quitOnLastWindowClose: true })` for macOS desktop apps. Closing
the final window now terminates the app when opted in; the default continues
to allow a windowless app to be reopened from the Dock. GTK4 and Windows
already exit when their last application window closes.
