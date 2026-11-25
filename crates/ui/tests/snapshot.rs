//! Captures de la vue de l'éditeur, rendues hors écran dans `target/cupride-snapshots/`.
//!
//! Elles vérifient que le rendu complet fonctionne de bout en bout, et servent à regarder le
//! résultat. Sur une machine sans GPU, les tests sont ignorés.

use cupride_core::Document;
use cupride_core::Editor;
use cupride_core::text::{Selection, Selections};
use cupride_render::{Gpu, OFFSCREEN_FORMAT, OffscreenTarget, Rect, Renderer, TextSystem};
use cupride_ui::{
    CommandLine, EditorView, MessageKind, StatusBar, StatusContext, StatusElements, Theme, Toasts,
    ViewSettings,
};

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
    let mut editor = Editor::new();
    editor.insert_document(Document::from_text(&"let x = 1;\n".repeat(20)));
    editor.notify(
        cupride_core::notifications::Level::Info,
        "src/main.rs enregistré",
    );
    editor.notify(
        cupride_core::notifications::Level::Warning,
        "barre d'état : élément inconnu « fichier » ignoré",
    );
    let document = editor.active_document().expect("document");
    let mut view = EditorView::new(ViewSettings::default(), 1.0);
    let window = Rect::new(0.0, 0.0, width as f32, height as f32);
    let style = view.style();
    let status_height = StatusBar::height(&style);
    view.set_bounds(Rect::new(
        0.0,
        0.0,
        window.width,
        window.height - status_height,
    ));
    view.prepare(&mut text, document);
    let theme = Theme::default();
    let mut status = StatusBar::default();
    let config = editor.config().status_bar.clone();
    status.prepare(
        &mut text,
        &style,
        window,
        &StatusElements::default(),
        &StatusContext {
            editor: &editor,
            pending_keys: "",
            panel: None,
        },
        (&config.left, &config.right),
    );
    let mut line = CommandLine::new("file opne src/main.rs");
    line.set_message(
        MessageKind::Error,
        "commande inconnue : « file opne » (vouliez-vous dire « file open » ?)",
    );
    line.prepare(&mut text, &style, window, status_height);
    let mut frame = cupride_render::Frame::new(theme.background);
    view.draw(frame.base(), &theme, document, false);
    status.draw(frame.base(), &style, &theme);
    let mut toasts = Toasts::default();
    toasts.sync(
        editor.notifications(),
        &editor.config().notifications,
        std::time::Instant::now(),
    );
    toasts.prepare(
        &mut text,
        &style,
        Rect::new(0.0, 0.0, window.width, line.area().y),
    );
    line.draw(frame.push_layer(), &style, &theme, true);
    toasts.draw(frame.push_layer(), &style, &theme);
    renderer
        .render(&gpu, &mut text, &target.view, target.size(), &frame)
        .expect("rendu");
    let pixels = target.read_rgba(&gpu).expect("relecture de l'image");
    save_png("ligne_de_commande", (width, height), &pixels);
}

/// Les trois thèmes intégrés, côte à côte dans des captures séparées.
#[test]
fn themes_integres() {
    for id in ["padparadscha", "clair", "sombre"] {
        let theme = cupride_ui::Themes::default()
            .get(id)
            .cloned()
            .expect("thème intégré");
        render_with_theme(&format!("theme_{id}"), &theme);
    }
}

fn render_with_theme(name: &str, theme: &Theme) {
    let gpu = match Gpu::headless() {
        Ok(gpu) => gpu,
        Err(error) => {
            eprintln!("test ignoré, pas de GPU : {error}");
            return;
        }
    };
    let (width, height) = (480, 120);
    let target = OffscreenTarget::new(&gpu, width, height);
    let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
    let mut text = TextSystem::new();
    let mut document = Document::from_text("fn main() {\n    let x = \"thème\";\n}\n");
    document.set_selections(Selections::single(Selection::new(20, 25)));
    let mut view = EditorView::new(ViewSettings::default(), 1.0);
    let style = view.style();
    let window = Rect::new(0.0, 0.0, width as f32, height as f32);
    view.set_bounds(Rect::new(
        0.0,
        0.0,
        window.width,
        window.height - StatusBar::height(&style),
    ));
    let mut editor = Editor::new();
    editor.insert_document(document);
    let document = editor.active_document().expect("document");
    view.prepare(&mut text, document);
    let mut status = StatusBar::default();
    let config = editor.config().status_bar.clone();
    status.prepare(
        &mut text,
        &style,
        window,
        &StatusElements::default(),
        &StatusContext {
            editor: &editor,
            pending_keys: "",
            panel: None,
        },
        (&config.left, &config.right),
    );
    let mut frame = cupride_render::Frame::new(theme.background);
    view.draw(frame.base(), theme, document, true);
    status.draw(frame.base(), &style, theme);
    renderer
        .render(&gpu, &mut text, &target.view, target.size(), &frame)
        .expect("rendu");
    let pixels = target.read_rgba(&gpu).expect("relecture de l'image");
    save_png(name, (width, height), &pixels);
}

/// Palette de commandes ouverte sur « fop », avec les vraies commandes.
#[test]
fn palette() {
    let gpu = match Gpu::headless() {
        Ok(gpu) => gpu,
        Err(error) => {
            eprintln!("test ignoré, pas de GPU : {error}");
            return;
        }
    };
    let (width, height) = (800, 420);
    let target = OffscreenTarget::new(&gpu, width, height);
    let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
    let mut text = TextSystem::new();
    let editor = Editor::new();
    let items: Vec<cupride_ui::PickerItem> = editor
        .commands()
        .list()
        .map(|c| cupride_ui::PickerItem {
            label: c.full_name(),
            detail: c.description().to_owned(),
            hint: editor
                .keymap()
                .keys_for(&c.full_name())
                .next()
                .map(ToString::to_string)
                .unwrap_or_default(),
        })
        .collect();
    let mut picker = cupride_ui::Picker::new("Commande…", items, vec![], false);
    picker.input.insert("fo");
    let theme = Theme::default();
    let view = EditorView::new(ViewSettings::default(), 1.0);
    let style = view.style();
    let window = Rect::new(0.0, 0.0, width as f32, height as f32);
    picker.prepare(&mut text, &style, &theme, window);
    let mut frame = cupride_render::Frame::new(theme.background);
    picker.draw(frame.push_layer(), &style, &theme, true);
    renderer
        .render(&gpu, &mut text, &target.view, target.size(), &frame)
        .expect("rendu");
    let pixels = target.read_rgba(&gpu).expect("relecture de l'image");
    save_png("palette", (width, height), &pixels);
}

/// Fenêtre divisée : deux zones sur le même document (curseurs propres à chacune), des onglets,
/// un onglet modifié et un onglet d'aperçu.
#[test]
fn zones_et_onglets() {
    use cupride_core::NewPanel;
    use cupride_core::layout::Direction;
    use cupride_render::Frame;
    use cupride_ui::{PanelViews, ViewRegistry};

    let gpu = match Gpu::headless() {
        Ok(gpu) => gpu,
        Err(error) => {
            eprintln!("test ignoré, pas de GPU : {error}");
            return;
        }
    };
    let (width, height) = (900_u32, 300_u32);
    let target = OffscreenTarget::new(&gpu, width, height);
    let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
    let mut text = TextSystem::new();
    let mut editor = Editor::new();
    let mut config = editor.config().clone();
    config.tabs.preview = true;
    editor.apply_config(config);
    let doc = editor.insert_document(Document::from_text(
        "fn main() {\n    let message = \"bonjour\";\n    println!(\"{message}\");\n}\n",
    ));
    editor.execute("edit move doc-end").unwrap();
    editor
        .open_editor_panel(doc, NewPanel::Split(Direction::Right))
        .unwrap();
    editor.execute("edit insert '// modifié'").unwrap();
    let other = editor.create_document();
    editor.show_document_preview(other);
    editor.show_document(doc);

    let mut views = PanelViews::new(ViewRegistry::default(), ViewSettings::default(), 1.0);
    let theme = Theme::default();
    views.layout(
        &mut text,
        &editor,
        Rect::new(0.0, 0.0, width as f32, height as f32),
    );
    views.prepare(&mut text, &editor, &theme, true, true);
    let mut frame = Frame::new(theme.background);
    views.draw(frame.base(), &editor, &theme, true, true);
    renderer
        .render(&gpu, &mut text, &target.view, target.size(), &frame)
        .expect("rendu");
    let pixels = target.read_rgba(&gpu).expect("relecture de l'image");
    save_png("zones_et_onglets", (width, height), &pixels);
    assert_eq!(views.zones().len(), 2);
    assert_eq!(views.zones()[1].tabs.len(), 2);
}

/// Explorateur à gauche d'un éditeur : dossier déplié, fichier ignoré en gris, sélection.
#[test]
fn explorateur() {
    use cupride_render::Frame;
    use cupride_ui::{ExplorerView, PanelViews, ViewRegistry};

    let gpu = match Gpu::headless() {
        Ok(gpu) => gpu,
        Err(error) => {
            eprintln!("test ignoré, pas de GPU : {error}");
            return;
        }
    };
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join("src/util")).unwrap();
    std::fs::create_dir_all(root.join("target")).unwrap();
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.join("src/lib.rs"), "").unwrap();
    std::fs::write(root.join("Cargo.toml"), "").unwrap();
    std::fs::write(root.join(".gitignore"), "target/\n").unwrap();

    let (width, height) = (900_u32, 300_u32);
    let target = OffscreenTarget::new(&gpu, width, height);
    let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
    let mut text = TextSystem::new();
    let mut editor = Editor::new();
    editor.set_project(&root).unwrap();
    editor.open(root.join("src/main.rs")).unwrap();
    editor.execute("explorer show").unwrap();
    let explorer = editor.active_panel().unwrap();

    let mut views = PanelViews::new(ViewRegistry::default(), ViewSettings::default(), 1.0);
    let theme = Theme::default();
    let area = Rect::new(0.0, 0.0, width as f32, height as f32);
    views.layout(&mut text, &editor, area);
    views.prepare(&mut text, &editor, &theme, true, true);
    let view = views
        .view_mut(explorer)
        .unwrap()
        .as_any_mut()
        .downcast_mut::<ExplorerView>()
        .unwrap();
    view.reveal(&root.join("src/main.rs"));
    views.prepare(&mut text, &editor, &theme, true, true);
    let mut frame = Frame::new(theme.background);
    views.draw(frame.base(), &editor, &theme, true, true);
    renderer
        .render(&gpu, &mut text, &target.view, target.size(), &frame)
        .expect("rendu");
    let pixels = target.read_rgba(&gpu).expect("relecture de l'image");
    save_png("explorateur", (width, height), &pixels);
}

/// Recherche : occurrences surlignées, l'occurrence courante sélectionnée, barre en haut à droite.
#[test]
fn recherche() {
    use cupride_core::search::SearchQuery;
    use cupride_render::Frame;
    use cupride_ui::FindBar;

    let gpu = match Gpu::headless() {
        Ok(gpu) => gpu,
        Err(error) => {
            eprintln!("test ignoré, pas de GPU : {error}");
            return;
        }
    };
    let (width, height) = (800_u32, 200_u32);
    let target = OffscreenTarget::new(&gpu, width, height);
    let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
    let mut text = TextSystem::new();
    let mut editor = Editor::new();
    editor.insert_document(Document::from_text(
        "let message = \"bonjour\";\nprintln!(\"{message}\");\n// message de fin\n",
    ));
    editor.execute("search find message").unwrap();
    let id = editor.active_id().unwrap();
    let matches = std::sync::Arc::new(editor.search_matches(id).unwrap().to_vec());
    assert_eq!(matches.len(), 3);
    let document = editor.document(id).unwrap();

    let mut view = EditorView::new(ViewSettings::default(), 1.0);
    view.set_bounds(Rect::new(0.0, 0.0, width as f32, height as f32));
    view.set_highlights(matches);
    view.prepare(&mut text, document);
    let theme = Theme::default();
    let style = view.style();
    let mut frame = Frame::new(theme.background);
    view.draw(frame.base(), &theme, document, true);
    let mut bar = FindBar::new(&SearchQuery::new("message"));
    bar.set_status(Some("1/3".into()), false);
    bar.prepare(
        &mut text,
        &style,
        Rect::new(0.0, 0.0, width as f32, height as f32),
    );
    bar.draw(frame.push_layer(), &style, &theme, true);
    renderer
        .render(&gpu, &mut text, &target.view, target.size(), &frame)
        .expect("rendu");
    let pixels = target.read_rgba(&gpu).expect("relecture de l'image");
    save_png("recherche", (width, height), &pixels);
}

/// Panneau de recherche dans le projet, à côté d'un éditeur.
#[test]
fn recherche_dans_le_projet() {
    use cupride_core::search::SearchQuery;
    use cupride_render::Frame;
    use cupride_ui::{PanelViews, ViewRegistry};

    let gpu = match Gpu::headless() {
        Ok(gpu) => gpu,
        Err(error) => {
            eprintln!("test ignoré, pas de GPU : {error}");
            return;
        }
    };
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/main.rs"),
        "fn main() {\n    let message = 1;\n    dbg!(message);\n}\n",
    )
    .unwrap();
    std::fs::write(root.join("README.md"), "Le message du projet.\n").unwrap();
    let (width, height) = (900_u32, 260_u32);
    let target = OffscreenTarget::new(&gpu, width, height);
    let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
    let mut text = TextSystem::new();
    let mut editor = Editor::new();
    editor.set_project(&root).unwrap();
    editor.open(root.join("src/main.rs")).unwrap();
    editor
        .run_project_search(SearchQuery::new("message"))
        .unwrap();
    editor.show_side_panel("search", 0.4);

    let mut views = PanelViews::new(ViewRegistry::default(), ViewSettings::default(), 1.0);
    let theme = Theme::default();
    let area = Rect::new(0.0, 0.0, width as f32, height as f32);
    views.layout(&mut text, &editor, area);
    views.prepare(&mut text, &editor, &theme, true, true);
    let mut frame = Frame::new(theme.background);
    views.draw(frame.base(), &editor, &theme, true, true);
    renderer
        .render(&gpu, &mut text, &target.view, target.size(), &frame)
        .expect("rendu");
    let pixels = target.read_rgba(&gpu).expect("relecture de l'image");
    save_png("recherche_projet", (width, height), &pixels);
}

/// Retour à la ligne, espaces et fins de ligne visibles, règle à la colonne 40.
#[test]
fn affichage_des_lignes() {
    use cupride_ui::DisplayOptions;

    let gpu = match Gpu::headless() {
        Ok(gpu) => gpu,
        Err(error) => {
            eprintln!("test ignoré, pas de GPU : {error}");
            return;
        }
    };
    let (width, height) = (520_u32, 220_u32);
    let target = OffscreenTarget::new(&gpu, width, height);
    let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
    let mut text = TextSystem::new();
    let document = Document::from_text(
        "fn main() {\n\tlet phrase = \"une ligne assez longue pour revenir à la ligne dans la vue\";\n    println!(\"{phrase}\");\n}\n",
    );
    let mut view = EditorView::new(ViewSettings::default(), 1.0);
    view.set_bounds(Rect::new(0.0, 0.0, width as f32, height as f32));
    view.set_display(DisplayOptions {
        wrap: true,
        whitespace: true,
        line_endings: true,
        rulers: vec![40],
    });
    view.prepare(&mut text, &document);
    let frame = view.frame(&Theme::default(), &document, false);
    renderer
        .render(&gpu, &mut text, &target.view, target.size(), &frame)
        .expect("rendu");
    let pixels = target.read_rgba(&gpu).expect("relecture de l'image");
    save_png("affichage_des_lignes", (width, height), &pixels);
}
