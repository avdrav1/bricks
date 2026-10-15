//! Splitting an image's rows across the machine's cores, for the transforms that make a
//! whole new image ([`crate::Transform`]): each thread writes its own band of rows.

/// Below this many bytes of output one thread does it all: starting threads costs more.
const ALONE: usize = 1 << 20;

/// Calls `f(first, rows)` for bands of `out`'s rows, `row` bytes each, on as many threads
/// as there are cores: `rows` the band's bytes, `first` the number of its first row.
pub(crate) fn bands(out: &mut [u8], row: usize, f: impl Fn(u32, &mut [u8]) + Sync) {
    let height = out.len() / row;
    let threads = match out.len() < ALONE {
        true => 1,
        false => std::thread::available_parallelism().map_or(1, |n| n.get()),
    }
    .min(height)
    .max(1);
    if threads == 1 {
        f(0, out);
        return;
    }
    let per = height.div_ceil(threads);
    std::thread::scope(|scope| {
        for (k, band) in out.chunks_mut(per * row).enumerate() {
            let f = &f;
            scope.spawn(move || f((k * per) as u32, band));
        }
    });
}
