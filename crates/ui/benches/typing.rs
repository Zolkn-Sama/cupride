//! Latence de frappe, de la touche à l'image : insertion dans le document (commande
//! `edit insert`), mise en forme de la ligne modifiée, liste de dessin, puis rendu GPU complet,
//! en attendant que le GPU ait fini.
//!
//! N'inclut pas l'attente de la synchronisation verticale ni l'affichage par l'écran, qui ajoutent
//! jusqu'à une période de rafraîchissement (16,7 ms à 60 Hz). La latence réelle, jusqu'à la
//! présentation, est journalisée par l'application avec `CUPRIDE_LOG=cupride=debug`.
//!
//! Commande : `cargo bench -p cupride-ui --bench typing`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use cupride_core::text::Selections;
use cupride_core::{Document, Editor};
use cupride_render::{Gpu, OFFSCREEN_FORMAT, OffscreenTarget, Rect, Renderer, TextSystem};
use cupride_ui::{EditorView, Theme, ViewSettings};

const SCALE: f32 = 2.0;
const SIZE: (u32, u32) = (2_200, 1_440);

/// Éditeur sur un fichier de 10 000 lignes de code, curseur au milieu de l'écran.
fn editor() -> Editor {
    let line = "        let resultat = calculer(entree, &options).expect(\"échec\"); // 😀";
    let text = vec![line; 10_000].join("\n");
    let mut document = Document::from_text(&text);
    document.set_selections(Selections::cursor((line.len() + 1) * 15 + 30));
    let mut editor = Editor::new();
    editor.insert_document(document);
    editor
}

/// Une frappe : alternativement un caractère et un retour arrière, pour que la ligne garde sa
/// longueur d'une mesure à l'autre.
fn type_key(editor: &mut Editor, insert: &mut bool) {
    let result = if *insert {
        editor.run("edit insert", &["x".to_owned()])
    } else {
        editor.run("edit delete-backward", &[])
    };
    result.expect("frappe");
    *insert = !*insert;
}

fn view() -> EditorView {
    let mut view = EditorView::new(ViewSettings::default(), SCALE);
    view.set_bounds(Rect::new(0.0, 0.0, SIZE.0 as f32, SIZE.1 as f32));
    view
}

fn bench(c: &mut Criterion) {
    let theme = Theme::default();
    let mut text = TextSystem::new();
    let mut group = c.benchmark_group("frappe");

    let mut e = editor();
    let mut v = view();
    group.bench_function("insertion et preparation", |b| {
        let mut insert = true;
        b.iter(|| {
            type_key(&mut e, &mut insert);
            let document = e.active_document().expect("document");
            v.prepare(&mut text, document);
            black_box(v.frame(&theme, document, true).texts.len())
        });
    });

    if let Ok(gpu) = Gpu::headless() {
        let target = OffscreenTarget::new(&gpu, SIZE.0, SIZE.1);
        let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
        let mut e = editor();
        let mut v = view();
        let mut insert = true;
        group.bench_function("insertion et image complete (GPU)", |b| {
            b.iter(|| {
                type_key(&mut e, &mut insert);
                let document = e.active_document().expect("document");
                v.prepare(&mut text, document);
                let frame = v.frame(&theme, document, true);
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
