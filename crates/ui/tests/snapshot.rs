//! Captures de la vue de l'éditeur, rendues hors écran dans `target/cupride-snapshots/`.
//!
//! Elles vérifient que le rendu complet fonctionne de bout en bout, et servent à regarder le
//! résultat. Sur une machine sans GPU, les tests sont ignorés.

use cupride_core::Document;
use cupride_core::text::{Selection, Selections};
use cupride_render::{Gpu, OFFSCREEN_FORMAT, OffscreenTarget, Rect, Renderer, TextSystem};
use cupride_ui::{CommandLine, EditorView, MessageKind, Theme, ViewSettings};

fn save_png(name: &str, (width, height): (u32, u32), pixels: &[u8]) {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/cupride-snapshots");
    std::fs::create_dir_all(&dir).expect("dossier des captures");
    let file = std::fs::File::create(dir.join(format!("{name}.png"))).expect("fichier de capture");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(pixels))
        .expect("écriture de la capture");
}

#[test]
fn editeur_avec_selections_et_curseurs() {
    render_editor("editeur", 1.0);
}

/// Écran haute densité (Retina) : même vue, deux fois plus de pixels, texte mis en forme à cette
/// échelle (pas d'image agrandie).
#[test]
fn editeur_haute_densite() {
    render_editor("editeur_2x", 2.0);
}

fn render_editor(name: &str, scale: f32) {
    let gpu = match Gpu::headless() {
        Ok(gpu) => gpu,
        Err(error) => {
            eprintln!("test ignoré, pas de GPU : {error}");
            return;
        }
    };
    let (width, height) = ((640.0 * scale) as u32, (200.0 * scale) as u32);
    let target = OffscreenTarget::new(&gpu, width, height);
    let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
    let mut text = TextSystem::new();
    let mut document = Document::from_text(
        "fn main() {\n    let message = \"bonjour\";\n    println!(\"{message} 😀\");\n}\n// 日本語、中文、한국어\n",
    );
    let mut selections = Selections::single(Selection::new(20, 44));
    selections.push(Selection::cursor(54));
    document.set_selections(selections);

    let mut view = EditorView::new(ViewSettings::default(), scale);
    view.set_bounds(Rect::new(0.0, 0.0, width as f32, height as f32));
    view.prepare(&mut text, &document);
    let theme = Theme::default();
    let frame = view.frame(&theme, &document, true);
    renderer
        .render(&gpu, &mut text, &target.view, target.size(), &frame)
        .expect("rendu");
    let pixels = target.read_rgba(&gpu).expect("relecture de l'image");
    save_png(name, (width, height), &pixels);

    // Le curseur, opaque, apparaît avec sa couleur exacte.
    let cursor = [theme.cursor.r, theme.cursor.g, theme.cursor.b];
    let cursor_pixels = pixels
        .chunks(4)
        .filter(|p| p[..3].iter().zip(cursor).all(|(a, b)| a.abs_diff(b) <= 2))
        .count();
    assert!(
        cursor_pixels > 20,
        "curseur introuvable : {cursor_pixels} pixels"
    );
}

/// Ligne de commande ouverte par-dessus l'éditeur, avec un message d'erreur.
#[test]
fn ligne_de_commande() {
    let gpu = match Gpu::headless() {
        Ok(gpu) => gpu,
        Err(error) => {
            eprintln!("test ignoré, pas de GPU : {error}");
            return;
        }
    };
    let (width, height) = (640, 200);
    let target = OffscreenTarget::new(&gpu, width, height);
    let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
    let mut text = TextSystem::new();
    let document = Document::from_text(&"let x = 1;\n".repeat(20));
    let mut view = EditorView::new(ViewSettings::default(), 1.0);
    let window = Rect::new(0.0, 0.0, width as f32, height as f32);
    view.set_bounds(window);
    view.prepare(&mut text, &document);
    let theme = Theme::default();
    let style = view.style();
    let mut line = CommandLine::new("file opne src/main.rs");
    line.set_message(
        MessageKind::Error,
        "commande inconnue : « file opne » (vouliez-vous dire « file open » ?)",
    );
    line.prepare(&mut text, &style, window, 0.0);
    let mut frame = cupride_render::Frame::new(theme.background);
    view.draw(frame.base(), &theme, &document, false);
    line.draw(frame.push_layer(), &style, &theme, true);
    renderer
        .render(&gpu, &mut text, &target.view, target.size(), &frame)
        .expect("rendu");
    let pixels = target.read_rgba(&gpu).expect("relecture de l'image");
    save_png("ligne_de_commande", (width, height), &pixels);
}
