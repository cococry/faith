fn main() -> anyhow::Result<()> {
    let arguments: Vec<_> = std::env::args_os().collect();
    anyhow::ensure!(
        arguments.len() == 3,
        "Usage: faith-package-icon input output.png"
    );

    let image = image::open(&arguments[1])?
        .resize(512, 512, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    let mut icon = image::RgbaImage::new(512, 512);
    image::imageops::overlay(
        &mut icon,
        &image,
        i64::from((512 - image.width()) / 2),
        i64::from((512 - image.height()) / 2),
    );
    icon.save(&arguments[2])?;
    Ok(())
}
