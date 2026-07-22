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

#[test]
fn default_logo_mode_smooths_round_marks() {
    let image = RgbaImage::from_fn(32, 32, |x, y| {
        let dx = x as i32 - 16;
        let dy = y as i32 - 16;
        let dist_sq = dx * dx + dy * dy;
        if (70..=190).contains(&dist_sq) {
            Rgba([30, 30, 34, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });

    let svg = vectorize_fixture(
        image,
        VectorizeOptions {
            colors: 2,
            mode: VectorizeMode::Logo,
            ..VectorizeOptions::default()
        },
    );

    assert!(svg.contains(" C "), "default logo mode should curve round contours");
    assert_eq!(svg.matches("M ").count(), 2, "ring should keep outer and inner contours");
    assert!(
        svg.matches(" L ").count() < 4,
        "round logo contours should not be dominated by stair-step line segments"
    );
}

#[test]
fn logo_mode_removes_isolated_specks_by_default() {
    let image = RgbaImage::from_fn(8, 8, |x, y| {
        let in_mark = (1..=4).contains(&x) && (1..=4).contains(&y);
        let is_speck = (x, y) == (7, 0) || (x, y) == (0, 7);

        if in_mark || is_speck {
            Rgba([10, 10, 10, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });

    let svg = vectorize_fixture(
        image,
        VectorizeOptions {
            colors: 2,
            mode: VectorizeMode::Logo,
            ..VectorizeOptions::default()
        },
    );

    assert_eq!(svg.matches("<path").count(), 1);
}

#[test]
fn high_detail_logo_mode_preserves_tiny_components() {
    let image = RgbaImage::from_fn(8, 8, |x, y| {
        let in_mark = (1..=4).contains(&x) && (1..=4).contains(&y);
        let is_detail = (x, y) == (7, 0);

        if in_mark || is_detail {
            Rgba([10, 10, 10, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });

    let svg = vectorize_fixture(
        image,
        VectorizeOptions {
            colors: 2,
            detail: 0.9,
            mode: VectorizeMode::Logo,
            ..VectorizeOptions::default()
        },
    );

    assert_eq!(svg.matches("<path").count(), 2);
}

#[test]
fn logo_mode_merges_near_duplicate_brand_colors() {
    let image = RgbaImage::from_fn(8, 4, |x, _| {
        if x < 4 {
            Rgba([220, 40, 40, 255])
        } else {
            Rgba([225, 43, 39, 255])
        }
    });

    let svg = vectorize_fixture(
        image,
        VectorizeOptions {
            colors: 4,
            detail: 0.6,
            mode: VectorizeMode::Logo,
            ..VectorizeOptions::default()
        },
    );

    assert_eq!(svg.matches("<g fill=").count(), 1);
}

#[test]
fn pixel_mode_keeps_near_duplicate_colors_distinct() {
    let image = RgbaImage::from_fn(8, 4, |x, _| {
        if x < 4 {
            Rgba([220, 40, 40, 255])
        } else {
            Rgba([225, 43, 39, 255])
        }
    });

    let svg = vectorize_fixture(
        image,
        VectorizeOptions {
            colors: 4,
            detail: 1.0,
            mode: VectorizeMode::PixelArt,
            ..VectorizeOptions::default()
        },
    );

    assert_eq!(svg.matches("<g fill=").count(), 2);
}

#[test]
fn vectorization_output_is_deterministic() {
    let image = RgbaImage::from_fn(10, 10, |x, y| {
        if (1..=4).contains(&x) && (1..=4).contains(&y) {
            Rgba([220, 40, 40, 255])
        } else if (5..=8).contains(&x) && (5..=8).contains(&y) {
            Rgba([40, 90, 220, 255])
        } else if (x, y) == (8, 1) {
            Rgba([20, 20, 20, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });
    let options = VectorizeOptions {
        colors: 4,
        detail: 0.7,
        mode: VectorizeMode::Logo,
        ..VectorizeOptions::default()
    };
    let png = encode_png(&image);
    let first = png_to_svg(&png, &options).expect("fixture should vectorize");

    for _ in 0..10 {
        let next = png_to_svg(&png, &options).expect("fixture should vectorize");
        assert_eq!(first, next);
    }
}
