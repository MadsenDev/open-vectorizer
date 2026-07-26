//! Run Open Vectorizer on an arbitrary image file, for the shootout driver.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let image = image::open(&args[1]).unwrap().to_rgba8();
    let document = png2svg_core::vectorize_image(
        &image,
        &png2svg_core::VectorizeOptions {
            mode: png2svg_core::VectorizeMode::Logo,
            ..Default::default()
        },
    );
    std::fs::write(&args[2], document.to_svg()).unwrap();
}
