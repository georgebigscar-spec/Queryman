//! Шрифты. С feature `bundled-fonts` Inter и JetBrains Mono встраиваются в бинарник.

use eframe::egui;

#[cfg(feature = "bundled-fonts")]
pub fn install(ctx: &egui::Context) {
    use std::sync::Arc;
    const INTER: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
    const MONO: &[u8] = include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf");

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("inter".to_owned(), Arc::new(egui::FontData::from_static(INTER)));
    fonts.font_data.insert("jetbrains-mono".to_owned(), Arc::new(egui::FontData::from_static(MONO)));
    fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "inter".to_owned());
    fonts.families.entry(egui::FontFamily::Monospace).or_default().insert(0, "jetbrains-mono".to_owned());
    ctx.set_fonts(fonts);
}

#[cfg(not(feature = "bundled-fonts"))]
pub fn install(_ctx: &egui::Context) {}
