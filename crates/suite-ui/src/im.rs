//! Input methods for the suite's own text widgets (Slate's page, Frame's canvas, Etch's
//! canvas).
//!
//! GTK's macOS input method reads its client widget's window when the widget is set and
//! never again, and `GtkIMMulticontext` hands the widget on at once: set before the widget
//! is in a window, every key typed is dropped there. So the widget is set while it's
//! realized, as GTK's own `GtkText` does.

use gtk::prelude::*;

/// Make `widget` the client of `im` while it's realized.
pub fn attach(im: &impl IsA<gtk::IMContext>, widget: &impl IsA<gtk::Widget>) {
    let im = im.as_ref();
    if widget.is_realized() {
        im.set_client_widget(Some(widget));
    }
    widget.connect_realize({
        let im = im.clone();
        move |w| im.set_client_widget(Some(w))
    });
    widget.connect_unrealize({
        let im = im.clone();
        move |_| im.set_client_widget(None::<&gtk::Widget>)
    });
}
