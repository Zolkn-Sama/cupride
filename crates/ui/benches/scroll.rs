//! Défilement d'un fichier de 100 000 lignes : coût d'une image.
//!
//! Vue de la taille de la fenêtre par défaut sur un écran Retina (2 200 × 1 440 pixels physiques,
//! ~37 lignes visibles). Une image doit tenir largement dans 16,7 ms (60 Hz) pour défiler sans
//! saccade.
//!
//! Commande : `cargo bench -p cupride-ui`.

use std::hint::black_box;
use std::time::Instant;

use criterion::{Criterion, criterion_group, criterion_main};
use cupride_core::Document;
use cupride_render::{Gpu, OFFSCREEN_FORMAT, OffscreenTarget, Rect, Renderer, TextSystem};
use cupride_ui::{EditorView, Theme, ViewSettings};

const LINES: usize = 100_000;
const SCALE: f32 = 2.0;
const SIZE: (u32, u32) = (2_200, 1_440);

/// Code source réaliste : lignes de longueurs variées, indentation, ASCII et multi-octets.
fn source() -> String {
    let samples = [
        "fn traiter(entree: &str) -> Result<Vec<Jeton>, Erreur> {",
        "    let mut jetons = Vec::with_capacity(entree.len() / 4);",
        "    for (i, c) in entree.char_indices() {",
        "        // Caractère accentué : é, à, ç — et un emoji 😀",
        "        if c.is_whitespace() { continue; }",
        "        jetons.push(Jeton::new(i, c));",
        "    }",
        "    Ok(jetons)",
        "}",
        "",
    ];
    (0..LINES)
        .map(|i| samples[i % samples.len()])
        .collect::<Vec<_>>()
        .join("\n")
}

fn view() -> EditorView {
    let mut view = EditorView::new(ViewSettings::default(), SCALE);
    view.set_bounds(Rect::new(0.0, 0.0, SIZE.0 as f32, SIZE.1 as f32));
    view
}

fn bench(c: &mut Criterion) {
    let document = Document::from_text(&source());
    let theme = Theme::default();
    let mut text = TextSystem::new();
    let mut group = c.benchmark_group("defilement_100k_lignes");

    // Défilement continu : 3 lignes par image, 3 nouvelles lignes à mettre en forme.
    let mut v = view();
    let step = 3.0 * v.line_height();
    let mut down = true;
    group.bench_function("3 lignes (preparation)", |b| {
        b.iter(|| {
            v.scroll_by(0.0, if down { step } else { -step }, false, Instant::now());
            v.prepare(&mut text, &document);
            if v.visible_lines().end >= LINES {
                down = false;
            } else if v.visible_lines().start == 0 {
                down = true;
            }
            black_box(v.frame(&theme, &document, true).texts.len())
        });
    });

    // Saut d'une page : toutes les lignes visibles sont nouvelles.
    let mut v = view();
    let page = SIZE.1 as f32;
    group.bench_function("1 page (preparation)", |b| {
        b.iter(|| {
            // En bas du fichier, retour au début : chaque image montre une page nouvelle.
            let dy = if v.visible_lines().end >= LINES {
                -1e12
            } else {
                page
            };
            v.scroll_by(0.0, dy, false, Instant::now());
            v.prepare(&mut text, &document);
            black_box(v.frame(&theme, &document, true).texts.len())
        });
    });

    // Image complète, GPU compris (attente de la fin du rendu).
    if let Ok(gpu) = Gpu::headless() {
        let target = OffscreenTarget::new(&gpu, SIZE.0, SIZE.1);
        let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
        let mut v = view();
        let mut down = true;
        group.bench_function("3 lignes (image complete, GPU)", |b| {
            b.iter(|| {
                v.scroll_by(0.0, if down { step } else { -step }, false, Instant::now());
                v.prepare(&mut text, &document);
                if v.visible_lines().end >= LINES {
                    down = false;
                } else if v.visible_lines().start == 0 {
                    down = true;
                }
                let frame = v.frame(&theme, &document, true);
                renderer
                    .render(&gpu, &mut text, &target.view, SIZE, &frame)
                    .expect("rendu");
                gpu.device
                    .poll(cupride_render::wgpu::PollType::wait_indefinitely())
                    .expect("attente du GPU");
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
