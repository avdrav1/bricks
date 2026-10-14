//! Where the image sits in the canvas: its zoom and the widget position of its top-left
//! corner. Toolkit-free, so zooming and panning are tested without a display (ADR 0010).

/// The zoom steps Zoom In and Zoom Out go through, 10% to 3200% (docs/specs/etch.md).
pub const ZOOMS: [f64; 18] = [
    0.1,
    0.125,
    1.0 / 6.0,
    0.25,
    1.0 / 3.0,
    0.5,
    2.0 / 3.0,
    1.0,
    1.5,
    2.0,
    3.0,
    4.0,
    6.0,
    8.0,
    12.0,
    16.0,
    24.0,
    32.0,
];
pub const MIN_ZOOM: f64 = ZOOMS[0];
pub const MAX_ZOOM: f64 = ZOOMS[ZOOMS.len() - 1];
/// Backdrop kept around the image when it's fitted, and the least of it panning can't
/// take away: the image never leaves the window.
pub const MARGIN: f64 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// Widget pixels per image pixel.
    pub zoom: f64,
    /// The image's top-left corner in the widget.
    pub origin: (f64, f64),
    /// The widget's size.
    pub size: (f64, f64),
    /// The image's size in pixels.
    pub image: (f64, f64),
}

impl View {
    /// The image at 100%, centred, or fitted to the widget when it's larger.
    pub fn new(image: (u32, u32), size: (f64, f64)) -> View {
        let mut v = View {
            zoom: 1.0,
            origin: (0.0, 0.0),
            size,
            image: (image.0 as f64, image.1 as f64),
        };
        let fit = v.fit_zoom();
        v.zoom = fit.min(1.0);
        v.centre();
        v
    }

    /// The zoom that shows the whole image with [`MARGIN`] around it.
    fn fit_zoom(&self) -> f64 {
        let w = (self.size.0 - 2.0 * MARGIN).max(1.0) / self.image.0;
        let h = (self.size.1 - 2.0 * MARGIN).max(1.0) / self.image.1;
        w.min(h).clamp(MIN_ZOOM, MAX_ZOOM)
    }

    /// Fit to Window: the whole image, as large as the widget allows.
    pub fn fit(&mut self) {
        self.zoom = self.fit_zoom();
        self.centre();
    }

    /// Sets `zoom` (clamped to 10%–3200%), keeping the image point under `anchor` (widget
    /// coordinates) where it is, as far as the image can stay put.
    pub fn zoom_at(&mut self, zoom: f64, anchor: (f64, f64)) {
        let at = self.to_image(anchor);
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.origin = (anchor.0 - at.0 * self.zoom, anchor.1 - at.1 * self.zoom);
        self.clamp();
    }

    /// The next step of [`ZOOMS`] in (`dir` > 0) or out from the current zoom, around
    /// `anchor`. A zoom between steps (after Fit) goes to the step beyond it.
    pub fn step(&mut self, dir: i32, anchor: (f64, f64)) {
        let eps = 1e-9;
        let next = if dir > 0 {
            ZOOMS.iter().copied().find(|&z| z > self.zoom + eps)
        } else {
            ZOOMS.iter().rev().copied().find(|&z| z < self.zoom - eps)
        };
        if let Some(z) = next {
            self.zoom_at(z, anchor);
        }
    }

    /// Moves the image by `(dx, dy)` widget pixels, as far as it can go.
    pub fn pan(&mut self, dx: f64, dy: f64) {
        self.origin = (self.origin.0 + dx, self.origin.1 + dy);
        self.clamp();
    }

    /// The widget changed size: the image point at its centre stays at the centre.
    pub fn resize(&mut self, size: (f64, f64)) {
        let at = self.to_image((self.size.0 / 2.0, self.size.1 / 2.0));
        self.size = size;
        self.origin = (
            size.0 / 2.0 - at.0 * self.zoom,
            size.1 / 2.0 - at.1 * self.zoom,
        );
        self.clamp();
    }

    /// A widget point in image pixels (fractional; floor for the pixel).
    pub fn to_image(self, (x, y): (f64, f64)) -> (f64, f64) {
        (
            (x - self.origin.0) / self.zoom,
            (y - self.origin.1) / self.zoom,
        )
    }

    /// An image point in widget pixels.
    pub fn to_widget(self, (x, y): (f64, f64)) -> (f64, f64) {
        (self.origin.0 + x * self.zoom, self.origin.1 + y * self.zoom)
    }

    /// The image's size in widget pixels.
    pub fn shown(&self) -> (f64, f64) {
        (self.image.0 * self.zoom, self.image.1 * self.zoom)
    }

    fn centre(&mut self) {
        let (w, h) = self.shown();
        self.origin = ((self.size.0 - w) / 2.0, (self.size.1 - h) / 2.0);
        self.clamp();
    }

    /// Each way, an image narrower than the widget (less the margins) sits centred, and a
    /// wider one can move only until its edge is [`MARGIN`] inside the widget's.
    fn clamp(&mut self) {
        // Unrounded: the image point under the pointer stays put through any number of
        // zooms, and drawing hits the pixel shown.
        let axis = |origin: f64, shown: f64, size: f64| {
            if shown <= size - 2.0 * MARGIN {
                (size - shown) / 2.0
            } else {
                origin.clamp(size - MARGIN - shown, MARGIN)
            }
        };
        let (w, h) = self.shown();
        self.origin = (
            axis(self.origin.0, w, self.size.0),
            axis(self.origin.1, h, self.size.1),
        );
    }

    /// The zoom as the status bar shows it: `100%`, `12.5%`, `16.7%`.
    pub fn percent(&self) -> String {
        let p = (self.zoom * 1000.0).round() / 10.0;
        if p.fract() == 0.0 {
            format!("{p:.0}%")
        } else {
            format!("{p:.1}%")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (f64, f64) = (800.0, 600.0);

    fn close(a: (f64, f64), b: (f64, f64), tol: f64) -> bool {
        (a.0 - b.0).abs() <= tol && (a.1 - b.1).abs() <= tol
    }

    #[test]
    fn a_small_image_starts_at_100_percent_centred() {
        let v = View::new((400, 300), SIZE);
        assert_eq!(v.zoom, 1.0);
        assert_eq!(v.origin, (200.0, 150.0));
    }

    #[test]
    fn a_large_image_starts_fitted_with_a_margin() {
        let v = View::new((7680, 4320), SIZE);
        assert!(v.zoom < 1.0);
        let (w, h) = v.shown();
        assert!(w <= SIZE.0 - 2.0 * MARGIN + 1e-9 && h <= SIZE.1 - 2.0 * MARGIN + 1e-9);
        assert!(
            (w - (SIZE.0 - 2.0 * MARGIN)).abs() < 1e-6,
            "width is what limits"
        );
        assert!(close(v.to_widget((3840.0, 2160.0)), (400.0, 300.0), 1.0));
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        let mut v = View::new((2000, 1500), SIZE);
        v.zoom_at(1.0, (400.0, 300.0));
        let pointer = (523.0, 211.0);
        let under = v.to_image(pointer);
        for _ in 0..ZOOMS.len() {
            v.step(1, pointer);
            assert!(close(v.to_widget(under), pointer, 1e-6), "at {}", v.zoom);
        }
        assert_eq!(v.zoom, MAX_ZOOM);
        for _ in 0..3 {
            v.step(-1, pointer);
            assert!(close(v.to_widget(under), pointer, 1e-6), "at {}", v.zoom);
        }
        assert_eq!(v.zoom, 12.0);
    }

    #[test]
    fn zoom_steps_stop_at_10_and_3200_percent() {
        let mut v = View::new((100, 100), SIZE);
        for _ in 0..40 {
            v.step(-1, (0.0, 0.0));
        }
        assert_eq!(v.zoom, 0.1);
        for _ in 0..40 {
            v.step(1, (0.0, 0.0));
        }
        assert_eq!(v.zoom, 32.0);
        v.zoom_at(1000.0, (0.0, 0.0));
        assert_eq!(v.zoom, 32.0);
    }

    #[test]
    fn a_zoom_between_steps_goes_to_the_next_step() {
        let mut v = View::new((7000, 4000), SIZE);
        let fitted = v.zoom;
        assert!(!ZOOMS.contains(&fitted));
        v.step(1, (400.0, 300.0));
        assert_eq!(v.zoom, *ZOOMS.iter().find(|&&z| z > fitted).unwrap());
        let mut w = View::new((7000, 4000), SIZE);
        w.step(-1, (400.0, 300.0));
        assert_eq!(w.zoom, *ZOOMS.iter().rev().find(|&&z| z < fitted).unwrap());
    }

    #[test]
    fn panning_stops_before_the_image_leaves() {
        let mut v = View::new((2000, 1500), SIZE);
        v.zoom_at(1.0, (400.0, 300.0));
        v.pan(-1e6, -1e6);
        let (w, h) = v.shown();
        assert_eq!(
            (v.origin.0 + w, v.origin.1 + h),
            (SIZE.0 - MARGIN, SIZE.1 - MARGIN)
        );
        v.pan(1e6, 1e6);
        assert_eq!(v.origin, (MARGIN, MARGIN));
        v.pan(-37.0, -12.0);
        assert_eq!(v.origin, (MARGIN - 37.0, MARGIN - 12.0));
    }

    #[test]
    fn an_image_smaller_than_the_window_stays_centred() {
        let mut v = View::new((400, 300), SIZE);
        v.pan(100.0, -50.0);
        assert_eq!(v.origin, (200.0, 150.0));
        // Wide but short: pans sideways only.
        let mut v = View::new((2000, 100), SIZE);
        v.zoom_at(1.0, (400.0, 300.0));
        v.pan(-50.0, 80.0);
        assert_eq!(v.origin.1, 250.0);
    }

    #[test]
    fn resizing_keeps_the_centre_point() {
        let mut v = View::new((4000, 3000), SIZE);
        v.zoom_at(2.0, (400.0, 300.0));
        let centre = v.to_image((400.0, 300.0));
        v.resize((1000.0, 500.0));
        assert!(close(v.to_image((500.0, 250.0)), centre, 1e-6));
        assert_eq!(v.zoom, 2.0);
        let mut small = View::new((400, 300), SIZE);
        small.resize((1000.0, 900.0));
        assert_eq!(small.origin, (300.0, 300.0));
    }

    #[test]
    fn fit_shows_the_whole_image_even_enlarged() {
        let mut v = View::new((100, 50), SIZE);
        v.fit();
        assert_eq!(v.zoom, (SIZE.0 - 2.0 * MARGIN) / 100.0);
        assert_eq!(v.origin.0, MARGIN);
    }

    #[test]
    fn percentages_read_like_the_menu() {
        let mut v = View::new((10, 10), SIZE);
        let shown: Vec<String> = ZOOMS
            .iter()
            .map(|&z| {
                v.zoom = z;
                v.percent()
            })
            .collect();
        assert_eq!(
            shown,
            [
                "10%", "12.5%", "16.7%", "25%", "33.3%", "50%", "66.7%", "100%", "150%", "200%",
                "300%", "400%", "600%", "800%", "1200%", "1600%", "2400%", "3200%"
            ]
        );
    }
}
