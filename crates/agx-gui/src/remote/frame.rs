//! Captured window pixels and change detection. Pure data: no Slint, sockets or threads.

/// An RGBA8 (4 bytes per pixel, row-major, no padding) image of the whole window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

impl Frame {
    pub fn bounds(&self) -> Rect {
        Rect {
            x: 0,
            y: 0,
            w: self.width,
            h: self.height,
        }
    }

    fn row(&self, y: usize) -> &[u8] {
        &self.rgba[y * self.width * 4..(y + 1) * self.width * 4]
    }
}

/// Smallest rectangle containing every pixel that differs, or `None` when the frames
/// are identical. Both frames must have the same size.
pub fn changed_bounds(old: &Frame, new: &Frame) -> Option<Rect> {
    debug_assert_eq!((old.width, old.height), (new.width, new.height));
    let (mut top, mut bottom) = (None, 0);
    let (mut left, mut right) = (usize::MAX, 0);
    for y in 0..new.height {
        let (a, b) = (old.row(y), new.row(y));
        if a == b {
            continue;
        }
        top.get_or_insert(y);
        bottom = y;
        let first = a
            .chunks_exact(4)
            .zip(b.chunks_exact(4))
            .position(|(p, q)| p != q)
            .unwrap_or(0);
        let last = a
            .chunks_exact(4)
            .zip(b.chunks_exact(4))
            .rposition(|(p, q)| p != q)
            .unwrap_or(0);
        left = left.min(first);
        right = right.max(last);
    }
    top.map(|top| Rect {
        x: left,
        y: top,
        w: right - left + 1,
        h: bottom - top + 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank(width: usize, height: usize) -> Frame {
        Frame {
            width,
            height,
            rgba: vec![0; width * height * 4],
        }
    }

    fn paint(frame: &mut Frame, x: usize, y: usize) {
        frame.rgba[(y * frame.width + x) * 4] = 255;
    }

    #[test]
    fn identical_frames_have_no_changes() {
        assert_eq!(changed_bounds(&blank(8, 4), &blank(8, 4)), None);
    }

    #[test]
    fn bounds_cover_every_changed_pixel() {
        let old = blank(10, 6);
        let mut new = old.clone();
        paint(&mut new, 7, 1);
        paint(&mut new, 2, 4);
        assert_eq!(
            changed_bounds(&old, &new),
            Some(Rect {
                x: 2,
                y: 1,
                w: 6,
                h: 4
            })
        );
    }

    #[test]
    fn a_single_pixel_is_a_one_pixel_rectangle() {
        let old = blank(5, 5);
        let mut new = old.clone();
        paint(&mut new, 0, 0);
        assert_eq!(
            changed_bounds(&old, &new),
            Some(Rect {
                x: 0,
                y: 0,
                w: 1,
                h: 1
            })
        );
    }
}
