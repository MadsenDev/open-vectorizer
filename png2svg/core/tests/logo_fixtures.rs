use image::{codecs::png::PngEncoder, ColorType, ImageEncoder, Rgba, RgbaImage};
use png2svg_core::{png_to_svg, VectorizeMode, VectorizeOptions};

fn encode_png(image: &RgbaImage) -> Vec<u8> {
    let mut png_bytes = Vec::new();
    PngEncoder::new(&mut png_bytes)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ColorType::Rgba8.into(),
        )
        .expect("fixture image should encode");
    png_bytes
}

fn vectorize_fixture(image: RgbaImage, options: VectorizeOptions) -> String {
    png_to_svg(&encode_png(&image), &options).expect("fixture should vectorize")
}

#[test]
fn preserves_transparent_logo_cutouts() {
    let image = RgbaImage::from_fn(7, 7, |x, y| {
        let in_outer = (1..=5).contains(&x) && (1..=5).contains(&y);
        let in_hole = (3..=3).contains(&x) && (3..=3).contains(&y);

        if in_outer && !in_hole {
            Rgba([20, 20, 20, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });

    let svg = vectorize_fixture(
        image,
        VectorizeOptions {
            colors: 2,
            mode: VectorizeMode::PixelArt,
            ..VectorizeOptions::default()
        },
    );

    assert!(svg.contains("fill-rule=\"evenodd\""));
    assert_eq!(svg.matches("<path").count(), 1);
    assert_eq!(svg.matches("M ").count(), 2, "outer contour and cutout should be subpaths");
    assert!(!svg.contains("fill=\"#000000\" fill-opacity=\"0.000\""));
}

#[test]
fn keeps_two_color_logo_regions_separate() {
    let image = RgbaImage::from_fn(8, 4, |x, _| {
        if x < 4 {
            Rgba([220, 40, 40, 255])
        } else {
            Rgba([40, 90, 220, 255])
        }
    });

    let svg = vectorize_fixture(
        image,
        VectorizeOptions {
            colors: 2,
            detail: 1.0,
            mode: VectorizeMode::PixelArt,
            ..VectorizeOptions::default()
        },
    );

    assert_eq!(svg.matches("<g fill=").count(), 2);
    assert_eq!(svg.matches("<path").count(), 2);
}

#[test]
fn pixel_mode_keeps_crisp_cell_edges() {
    let image = RgbaImage::from_fn(4, 4, |x, y| {
        if x == y {
            Rgba([0, 0, 0, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });

    let svg = vectorize_fixture(
        image,
        VectorizeOptions {
            colors: 2,
            mode: VectorizeMode::PixelArt,
            ..VectorizeOptions::default()
        },
    );

    assert!(!svg.contains(" C "), "pixel mode should not emit smoothed curves");
    assert!(svg.contains(" L "), "pixel mode should use line segments");
}

#[test]
fn logo_mode_can_emit_smoothed_paths() {
    let image = RgbaImage::from_fn(6, 6, |x, y| {
        let near_diagonal = x == y || x + 1 == y || y + 1 == x;
        if near_diagonal {
            Rgba([10, 10, 10, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });

    let svg = vectorize_fixture(
        image,
        VectorizeOptions {
            colors: 2,
            smoothness: 0.8,
            tolerance: 0.5,
            mode: VectorizeMode::Logo,
            ..VectorizeOptions::default()
        },
    );

    assert!(svg.contains("<path"));
    assert!(svg.contains(" C "), "smooth logo mode should be allowed to use curves");
}
