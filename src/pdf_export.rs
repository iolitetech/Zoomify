//! A minimal, dependency-free single-page PDF wrapping a raster capture.
//!
//! This is deliberately not a vector export — [`crate::svg_export`] already
//! covers "the annotations as editable shapes" and is the one the roadmap
//! calls out as reusable elsewhere. PDF's actual job here is the other one:
//! a page that prints and attaches cleanly, the same flattened picture
//! Save/Copy already produce. Writing PDF structure by hand rather than
//! pulling in a crate keeps that job proportionate to what it's for.
//!
//! No compression filter is applied to the image stream — a real DEFLATE
//! encoder is a lot of code for a file that exists to be opened once and
//! printed or attached, not archived, so this trades file size for staying
//! self-contained. A 1920x1080 export is a few MB, which every PDF reader
//! handles without comment.

use std::io::Write as _;

/// Build a single-page PDF embedding `rgb` (top-down, 3 bytes per pixel, no
/// alpha) as a full-bleed image. The page is sized in points from `dpi` so
/// it prints at the screen's physical size rather than an arbitrary one.
pub fn build_pdf(width_px: u32, height_px: u32, rgb: &[u8], dpi: f32) -> Vec<u8> {
    debug_assert_eq!(rgb.len(), width_px as usize * height_px as usize * 3);

    let dpi = if dpi.is_finite() && dpi > 1.0 {
        dpi
    } else {
        96.0
    };
    let page_w = (width_px as f32 * 72.0 / dpi).max(1.0);
    let page_h = (height_px as f32 * 72.0 / dpi).max(1.0);

    let content = format!("q {} 0 0 {} 0 0 cm /Im0 Do Q", fnum(page_w), fnum(page_h));
    let content_bytes = content.as_bytes();

    let mut out: Vec<u8> = Vec::with_capacity(rgb.len() + 2048);
    out.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");

    // Offsets are recorded as each object is written, then folded into the
    // xref table at the end — the one thing about PDF structure that has to
    // come after the objects it points at.
    let mut offsets = [0usize; 6];

    offsets[1] = out.len();
    out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    offsets[2] = out.len();
    out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");

    offsets[3] = out.len();
    let _ = write!(
        out,
        "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] \
/Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>\nendobj\n",
        fnum(page_w),
        fnum(page_h),
    );

    offsets[4] = out.len();
    let _ = write!(
        out,
        "4 0 obj\n<< /Type /XObject /Subtype /Image /Width {w} /Height {h} \
/ColorSpace /DeviceRGB /BitsPerComponent 8 /Length {len} >>\nstream\n",
        w = width_px,
        h = height_px,
        len = rgb.len(),
    );
    out.extend_from_slice(rgb);
    out.extend_from_slice(b"\nendstream\nendobj\n");

    offsets[5] = out.len();
    let _ = write!(
        out,
        "5 0 obj\n<< /Length {} >>\nstream\n",
        content_bytes.len()
    );
    out.extend_from_slice(content_bytes);
    out.extend_from_slice(b"\nendstream\nendobj\n");

    let xref_offset = out.len();
    let _ = write!(out, "xref\n0 6\n0000000000 65535 f \n");
    for &off in &offsets[1..] {
        let _ = writeln!(out, "{:010} 00000 n ", off);
    }
    let _ = write!(
        out,
        "trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{}\n%%EOF",
        xref_offset
    );

    out
}

/// Drop the alpha channel and swap channel order: top-down BGRA in, top-down
/// RGB out. `render_to_capture`'s output is always BGRA with alpha forced to
/// 255, so there is nothing meaningful to composite — just discard it.
pub fn bgra_to_rgb(bgra: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(bgra.len() / 4 * 3);
    for px in bgra.as_chunks::<4>().0 {
        rgb.push(px[2]);
        rgb.push(px[1]);
        rgb.push(px[0]);
    }
    rgb
}

/// A handful of decimal places, trimmed to an integer when exact — plenty
/// for a page-size measurement in points and keeps the file readable.
fn fnum(v: f32) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r == r.trunc() {
        format!("{}", r as i64)
    } else {
        format!("{:.2}", r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bgra_to_rgb_drops_alpha_and_swaps_channel_order() {
        let bgra = [10u8, 20, 30, 255, 40, 50, 60, 128];
        let rgb = bgra_to_rgb(&bgra);
        assert_eq!(rgb, vec![30, 20, 10, 60, 50, 40]);
    }

    #[test]
    fn test_build_pdf_has_a_valid_header_and_trailer() {
        let rgb = vec![200u8; 4 * 3 * 3]; // 4x3 image, solid grey
        let pdf = build_pdf(4, 3, &rgb, 96.0);
        assert!(pdf.starts_with(b"%PDF-1.4"));
        let tail = String::from_utf8_lossy(&pdf[pdf.len().saturating_sub(40)..]);
        assert!(tail.trim_end().ends_with("%%EOF"));
        assert!(tail.contains("startxref"));
    }

    /// Byte-safe substring search. `pdf` embeds a raw image stream and a
    /// binary marker comment, so `String::from_utf8_lossy` is the wrong tool
    /// here: it substitutes invalid sequences with a multi-byte replacement
    /// character, which shifts every offset found in the lossy string out of
    /// step with the real byte positions in `pdf`.
    fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    #[test]
    fn test_build_pdf_image_stream_length_matches_the_declared_length() {
        let w = 5u32;
        let h = 5u32;
        let rgb = vec![7u8; (w * h * 3) as usize];
        let pdf = build_pdf(w, h, &rgb, 96.0);
        let len_marker = format!("/Length {}", rgb.len());
        assert!(
            find_bytes(&pdf, len_marker.as_bytes()).is_some(),
            "expected the image object to declare /Length {}",
            rgb.len()
        );
        // The declared byte count must actually appear as one contiguous
        // run right after that object's "stream\n" marker.
        let img_obj_start = find_bytes(&pdf, b"/Subtype /Image").unwrap();
        let stream_start = find_bytes(&pdf[img_obj_start..], b"stream\n").unwrap()
            + img_obj_start
            + b"stream\n".len();
        assert_eq!(&pdf[stream_start..stream_start + rgb.len()], rgb.as_slice());
    }

    #[test]
    fn test_page_size_in_points_follows_dpi() {
        // 192 px at 96 dpi is 2 inches, i.e. 144 points.
        let rgb = vec![0u8; 192 * 96 * 3];
        let pdf = build_pdf(192, 96, &rgb, 96.0);
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("[0 0 144 72]"));
    }

    #[test]
    fn test_xref_offsets_point_at_the_matching_object_headers() {
        let rgb = vec![1u8; 2 * 2 * 3];
        let pdf = build_pdf(2, 2, &rgb, 96.0);
        let entries_start = find_bytes(&pdf, b"xref\n0 6\n").unwrap() + b"xref\n0 6\n".len();
        // Skip the free-list head entry, then check each in-use offset.
        let free_len = "0000000000 65535 f \n".len();
        let mut pos = entries_start + free_len;
        for n in 1..=5 {
            let entry = std::str::from_utf8(&pdf[pos..pos + 20]).unwrap();
            let offset: usize = entry[..10].parse().unwrap();
            let expect_prefix = format!("{} 0 obj", n);
            assert!(
                pdf[offset..].starts_with(expect_prefix.as_bytes()),
                "object {n}'s xref offset does not point at its header"
            );
            pos += 20;
        }
    }
}
