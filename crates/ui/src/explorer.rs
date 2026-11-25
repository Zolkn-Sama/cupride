//! Explorateur de fichiers : arborescence du projet, chargée dossier par dossier à la demande.
//!
//! - [`ExplorerTree`] : le modèle, sans interface : dossiers dépliés, sélection, fichiers ignorés
//!   (`.gitignore`, `.ignore`), mise à jour d'un dossier qui a changé sur le disque.
//! - [`ExplorerView`] : la vue du panneau `explorer`. Tout se fait au clavier : flèches pour se
//!   déplacer, plier et déplier, Entrée pour ouvrir, F2 pour renommer, Suppr pour mettre à la
//!   corbeille, `n` et `N` pour créer un fichier ou un dossier, `m` pour déplacer. Les opérations
//!   sur les fichiers passent par les commandes `explorer …`, préremplies dans la ligne de
//!   commande : Entrée les confirme.

use std::any::Any;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

use cupride_core::command::quote;
use cupride_core::config::IgnoredFiles;
use cupride_core::keymap::{Key, KeyCombo, Modifiers, NamedKey};
use cupride_core::layout::PanelId;
use cupride_render::{Layer, Quad, Rect, TextSystem};

use crate::editor_view::{Pointer, ViewSettings};
use crate::label::Label;
use crate::panels::{PanelContext, PanelView, ViewAction};
use crate::style::Style;

/// Entrée d'un dossier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Chemin complet.
    pub path: PathBuf,
    /// Nom.
    pub name: String,
    /// Dossier (ou lien vers un dossier).
    pub is_dir: bool,
    /// Ignoré par `.gitignore` ou `.ignore`.
    pub ignored: bool,
}

/// Ligne affichée de l'arborescence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Entrée affichée.
    pub entry: Entry,
    /// Profondeur : 0 pour les entrées du dossier racine.
    pub depth: usize,
    /// Dossier déplié.
    pub expanded: bool,
}

/// Arborescence d'un dossier, chargée à la demande.
#[derive(Debug, Clone)]
pub struct ExplorerTree {
    root: PathBuf,
    mode: IgnoredFiles,
    /// Contenu des dossiers déjà lus.
    listings: HashMap<PathBuf, Vec<Entry>>,
    /// Dossiers dépliés (la racine l'est toujours).
    expanded: BTreeSet<PathBuf>,
    selected: Option<PathBuf>,
    rows: Vec<Row>,
}

/// Lit le dossier `dir` : dossiers d'abord, puis fichiers, par ordre alphabétique sans tenir
/// compte de la casse. `.git` n'est jamais listé.
fn list(dir: &Path) -> io::Result<Vec<Entry>> {
    // Entrées que les règles d'exclusion laissent passer : les autres sont ignorées.
    let kept: HashSet<PathBuf> = ignore::WalkBuilder::new(dir)
        .max_depth(Some(1))
        .hidden(false)
        .parents(true)
        .require_git(false)
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.depth() == 1)
        .map(ignore::DirEntry::into_path)
        .collect();
    let mut entries = Vec::new();
    for item in fs::read_dir(dir)? {
        let Ok(item) = item else {
            continue;
        };
        let name = item.file_name().to_string_lossy().into_owned();
        if name == ".git" {
            continue;
        }
        let path = item.path();
        // Un lien est suivi : un lien vers un dossier se déplie.
        let is_dir = fs::metadata(&path).is_ok_and(|m| m.is_dir());
        let ignored = !kept.contains(&path);
        entries.push(Entry {
            path,
            name,
            is_dir,
            ignored,
        });
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(entries)
}

impl ExplorerTree {
    /// Arborescence de `root`, dont seul le premier niveau est lu.
    pub fn new(root: impl Into<PathBuf>, mode: IgnoredFiles) -> Self {
        let root = root.into();
        let mut tree = Self {
            listings: HashMap::new(),
            expanded: BTreeSet::new(),
            selected: None,
            rows: Vec::new(),
            root: root.clone(),
            mode,
        };
        tree.load(&root);
        tree.rebuild();
        tree.selected = tree.rows.first().map(|r| r.entry.path.clone());
        tree
    }

    /// Dossier racine.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Lignes affichées.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Entrée sélectionnée.
    pub fn selected(&self) -> Option<&Path> {
        self.selected.as_deref()
    }

    /// Rang de la ligne sélectionnée.
    pub fn selected_index(&self) -> Option<usize> {
        let selected = self.selected.as_ref()?;
        self.rows.iter().position(|r| &r.entry.path == selected)
    }

    /// Ligne sélectionnée.
    pub fn selected_row(&self) -> Option<&Row> {
        self.rows.get(self.selected_index()?)
    }

    /// Change l'affichage des fichiers ignorés.
    pub fn set_mode(&mut self, mode: IgnoredFiles) {
        if mode != self.mode {
            self.mode = mode;
            self.rebuild();
            self.keep_selection(0);
        }
    }

    /// Dossiers lus et dépliés : ceux à surveiller sur le disque.
    pub fn watched_dirs(&self) -> Vec<PathBuf> {
        std::iter::once(self.root.clone())
            .chain(
                self.expanded
                    .iter()
                    .filter(|d| self.listings.contains_key(*d))
                    .cloned(),
            )
            .collect()
    }

    /// Lit le dossier `dir` ; une erreur (dossier supprimé, illisible) le laisse vide.
    fn load(&mut self, dir: &Path) {
        let entries = list(dir).unwrap_or_else(|error| {
            tracing::debug!(dossier = %dir.display(), "lecture impossible : {error}");
            Vec::new()
        });
        self.listings.insert(dir.to_owned(), entries);
    }

    /// Recalcule les lignes affichées.
    fn rebuild(&mut self) {
        fn walk(tree: &ExplorerTree, dir: &Path, depth: usize, rows: &mut Vec<Row>) {
            let Some(entries) = tree.listings.get(dir) else {
                return;
            };
            for entry in entries {
                if entry.ignored && tree.mode == IgnoredFiles::Hide {
                    continue;
                }
                let expanded = entry.is_dir && tree.expanded.contains(&entry.path);
                rows.push(Row {
                    entry: entry.clone(),
                    depth,
                    expanded,
                });
                if expanded {
                    walk(tree, &entry.path, depth + 1, rows);
                }
            }
        }
        let mut rows = Vec::new();
        walk(self, &self.root, 0, &mut rows);
        self.rows = rows;
    }

    /// Garde une sélection valide : l'entrée sélectionnée si elle est encore affichée, sinon la
    /// ligne au rang `fallback` (ou la dernière).
    fn keep_selection(&mut self, fallback: usize) {
        if self.selected_index().is_some() {
            return;
        }
        let index = fallback.min(self.rows.len().saturating_sub(1));
        self.selected = self.rows.get(index).map(|r| r.entry.path.clone());
    }

    /// Sélectionne `path` s'il est affiché.
    pub fn select(&mut self, path: &Path) -> bool {
        if self.rows.iter().any(|r| r.entry.path == path) {
            self.selected = Some(path.to_owned());
            true
        } else {
            false
        }
    }

    /// Déplace la sélection de `delta` lignes, bornée à la première et à la dernière.
    pub fn move_selection(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let index = self.selected_index().unwrap_or(0) as isize + delta;
        let index = index.clamp(0, self.rows.len() as isize - 1) as usize;
        self.selected = Some(self.rows[index].entry.path.clone());
    }

    /// Déplie le dossier `dir` (lu à la première ouverture).
    pub fn expand(&mut self, dir: &Path) {
        if !self.listings.contains_key(dir) {
            self.load(dir);
        }
        self.expanded.insert(dir.to_owned());
        self.rebuild();
    }

    /// Replie le dossier `dir` ; une sélection à l'intérieur remonte sur lui.
    pub fn collapse(&mut self, dir: &Path) {
        self.expanded.remove(dir);
        if self.selected.as_ref().is_some_and(|s| s.starts_with(dir)) {
            self.selected = Some(dir.to_owned());
        }
        self.rebuild();
    }

    /// Plie ou déplie le dossier `dir`.
    pub fn toggle(&mut self, dir: &Path) {
        if self.expanded.contains(dir) {
            self.collapse(dir);
        } else {
            self.expand(dir);
        }
    }

    /// Flèche droite : déplie le dossier sélectionné, ou descend sur son premier enfant.
    pub fn right(&mut self) {
        let Some(row) = self.selected_row().cloned() else {
            return;
        };
        if !row.entry.is_dir {
            return;
        }
        if row.expanded {
            self.move_selection(1);
            if self.selected_row().is_none_or(|r| r.depth <= row.depth) {
                self.selected = Some(row.entry.path);
            }
        } else {
            self.expand(&row.entry.path);
        }
    }

    /// Flèche gauche : replie le dossier sélectionné, ou remonte sur le dossier parent.
    pub fn left(&mut self) {
        let Some(row) = self.selected_row().cloned() else {
            return;
        };
        if row.expanded {
            self.collapse(&row.entry.path);
        } else if let Some(parent) = row.entry.path.parent()
            && parent != self.root
        {
            self.selected = Some(parent.to_owned());
        }
    }

    /// Relit les dossiers déjà lus qui contiennent `paths` (ou sont dans `paths`) : le disque a
    /// changé. Les dossiers disparus sont oubliés ; la sélection reste sur son entrée, ou sur la
    /// ligne qui prend sa place.
    pub fn refresh(&mut self, paths: &[PathBuf]) {
        let fallback = self.selected_index().unwrap_or(0);
        let mut dirs: BTreeSet<PathBuf> = BTreeSet::new();
        for path in paths {
            if self.listings.contains_key(path) {
                dirs.insert(path.clone());
            }
            if let Some(parent) = path.parent()
                && self.listings.contains_key(parent)
            {
                dirs.insert(parent.to_owned());
            }
        }
        if dirs.is_empty() {
            return;
        }
        for dir in &dirs {
            if dir != &self.root && !dir.is_dir() {
                self.listings.retain(|d, _| !d.starts_with(dir));
                self.expanded.retain(|d| !d.starts_with(dir));
            } else {
                self.load(dir);
            }
        }
        // Les dossiers lus qui ont disparu de leur parent sont oubliés.
        let present: HashSet<PathBuf> = self
            .listings
            .values()
            .flatten()
            .map(|e| e.path.clone())
            .collect();
        let root = self.root.clone();
        self.listings
            .retain(|d, _| *d == root || present.contains(d));
        self.expanded.retain(|d| present.contains(d));
        self.rebuild();
        self.keep_selection(fallback);
    }

    /// Montre `path` : ses dossiers parents sont dépliés et il est sélectionné. Renvoie faux s'il
    /// n'est pas dans l'arborescence.
    pub fn reveal(&mut self, path: &Path) -> bool {
        let Ok(relative) = path.strip_prefix(&self.root) else {
            return false;
        };
        let mut dir = self.root.clone();
        let components: Vec<_> = relative.components().collect();
        for component in components.iter().take(components.len().saturating_sub(1)) {
            dir.push(component);
            if !self.listings.contains_key(&dir) {
                self.load(&dir);
            }
            self.expanded.insert(dir.clone());
        }
        // Le dossier parent est relu : l'entrée vient peut-être d'être créée.
        let parent = path.parent().unwrap_or(&self.root).to_owned();
        self.load(&parent);
        self.rebuild();
        self.select(path)
    }

    /// Chemin à afficher dans une commande : relatif à la racine quand il est dedans.
    ///
    /// Un chemin relatif s'écrit avec `/`, sur tous les OS (Windows l'accepte) : la commande reste
    /// lisible, sans `\` doublés par les guillemets.
    pub fn display_path(&self, path: &Path) -> String {
        match path
            .strip_prefix(&self.root)
            .ok()
            .filter(|p| !p.as_os_str().is_empty())
        {
            Some(relative) => relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/"),
            None => path.to_string_lossy().into_owned(),
        }
    }

    /// Dossier où créer une entrée : le dossier sélectionné, sinon celui de l'entrée
    /// sélectionnée, sinon la racine.
    pub fn target_dir(&self) -> PathBuf {
        match self.selected_row() {
            Some(row) if row.entry.is_dir => row.entry.path.clone(),
            Some(row) => row
                .entry
                .path
                .parent()
                .map_or_else(|| self.root.clone(), Path::to_path_buf),
            None => self.root.clone(),
        }
    }
}

/// Vue du panneau `explorer`.
pub struct ExplorerView {
    panel: PanelId,
    settings: ViewSettings,
    scale: f32,
    bounds: Rect,
    tree: Option<ExplorerTree>,
    /// Première ligne affichée, en pixels de défilement.
    scroll: f32,
    /// La sélection doit être rendue visible au prochain dessin.
    follow: bool,
    /// Chemin à montrer dès que l'arborescence existe.
    pending_reveal: Option<PathBuf>,
    labels: HashMap<PathBuf, Label>,
}

impl std::fmt::Debug for ExplorerView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExplorerView")
            .field("panel", &self.panel)
            .field("root", &self.tree.as_ref().map(ExplorerTree::root))
            .finish_non_exhaustive()
    }
}

impl ExplorerView {
    /// Vue de l'explorateur du panneau `panel`.
    pub fn new(panel: PanelId, settings: ViewSettings, scale: f32) -> Self {
        Self {
            panel,
            settings,
            scale,
            bounds: Rect::default(),
            tree: None,
            scroll: 0.0,
            follow: true,
            pending_reveal: None,
            labels: HashMap::new(),
        }
    }

    /// Arborescence, une fois la vue préparée.
    pub fn tree(&self) -> Option<&ExplorerTree> {
        self.tree.as_ref()
    }

    /// Arborescence, modifiable.
    pub fn tree_mut(&mut self) -> Option<&mut ExplorerTree> {
        self.follow = true;
        self.tree.as_mut()
    }

    /// Montre `path` dans l'arborescence (dossiers parents dépliés, entrée sélectionnée).
    pub fn reveal(&mut self, path: &Path) {
        match &mut self.tree {
            Some(tree) => {
                tree.reveal(path);
                self.follow = true;
            }
            None => self.pending_reveal = Some(path.to_owned()),
        }
    }

    fn style(&self) -> Style {
        self.settings.style(self.scale)
    }

    /// Hauteur d'une ligne.
    pub fn row_height(&self) -> f32 {
        let style = self.style();
        style.line_height + style.px(4.0)
    }

    fn padding(&self) -> f32 {
        self.style().px(6.0)
    }

    /// Lignes entièrement visibles.
    fn page_rows(&self) -> usize {
        ((self.bounds.height - 2.0 * self.padding()) / self.row_height().max(1.0)).max(1.0) as usize
    }

    /// Rang de la ligne sous l'ordonnée `y`.
    fn row_at(&self, y: f32) -> Option<usize> {
        let offset = y - self.bounds.y - self.padding() + self.scroll;
        if offset < 0.0 {
            return None;
        }
        let index = (offset / self.row_height().max(1.0)) as usize;
        let rows = self.tree.as_ref()?.rows().len();
        (index < rows).then_some(index)
    }

    /// Défilement maximal.
    fn max_scroll(&self) -> f32 {
        let rows = self.tree.as_ref().map_or(0, |t| t.rows().len()) as f32;
        (rows * self.row_height() + 2.0 * self.padding() - self.bounds.height).max(0.0)
    }

    /// Fait défiler juste assez pour montrer la sélection.
    fn reveal_selection(&mut self) {
        let Some(index) = self.tree.as_ref().and_then(ExplorerTree::selected_index) else {
            return;
        };
        let height = self.row_height();
        let top = index as f32 * height;
        let visible = self.bounds.height - 2.0 * self.padding();
        if top < self.scroll {
            self.scroll = top;
        } else if top + height > self.scroll + visible {
            self.scroll = top + height - visible;
        }
    }

    /// Ligne de commande préremplie pour agir sur la sélection.
    fn command_line(&self, command: &str, arguments: &[String]) -> ViewAction {
        let mut line = format!("explorer {command}");
        for argument in arguments {
            line.push(' ');
            line.push_str(&quote(argument));
        }
        ViewAction::CommandLine(line)
    }

    /// Ouvre le fichier `path`, en aperçu ou non.
    fn open(path: &Path, preview: bool) -> ViewAction {
        let path = quote(&path.to_string_lossy());
        ViewAction::Run(if preview {
            format!("file open --preview {path}")
        } else {
            format!("file open {path}")
        })
    }

    /// Entrée (ou double clic) sur la ligne sélectionnée.
    fn activate(&mut self, preview: bool) -> ViewAction {
        let Some(tree) = &mut self.tree else {
            return ViewAction::Ignored;
        };
        let Some(row) = tree.selected_row().cloned() else {
            return ViewAction::Handled;
        };
        if row.entry.is_dir {
            tree.toggle(&row.entry.path);
            ViewAction::Handled
        } else {
            Self::open(&row.entry.path, preview)
        }
    }
}

/// La combinaison `combo` est-elle la touche `key`, avec seulement les modificateurs `shift` ?
fn is(combo: &KeyCombo, key: Key, shift: bool) -> bool {
    combo.key == key
        && combo.modifiers
            == Modifiers {
                shift,
                ..Modifiers::NONE
            }
}

impl PanelView for ExplorerView {
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn set_settings(&mut self, settings: &ViewSettings) {
        if settings != &self.settings {
            self.settings = settings.clone();
            self.labels.clear();
        }
    }

    fn set_scale(&mut self, scale: f32) {
        if (scale - self.scale).abs() > f32::EPSILON && scale > 0.0 {
            self.scroll *= scale / self.scale;
            self.scale = scale;
            self.labels.clear();
        }
    }

    fn prepare(&mut self, text: &mut TextSystem, cx: &PanelContext<'_>) {
        let root = cx
            .editor
            .project()
            .map(Path::to_path_buf)
            .or_else(|| std::env::current_dir().ok());
        let mode = cx.editor.config().explorer.ignored;
        if let Some(root) = root
            && self.tree.as_ref().is_none_or(|t| t.root() != root)
        {
            self.tree = Some(ExplorerTree::new(root, mode));
            self.scroll = 0.0;
            self.follow = true;
        }
        let Some(tree) = &mut self.tree else {
            return;
        };
        tree.set_mode(mode);
        if let Some(path) = self.pending_reveal.take() {
            tree.reveal(&path);
            self.follow = true;
        }
        if std::mem::take(&mut self.follow) {
            self.reveal_selection();
        }
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
        // Seules les lignes visibles sont mises en forme.
        let style = self.style();
        let height = self.row_height().max(1.0);
        let first = (self.scroll / height) as usize;
        let count = (self.bounds.height / height).ceil() as usize + 1;
        let Some(tree) = &self.tree else {
            return;
        };
        let visible: Vec<&Row> = tree.rows().iter().skip(first).take(count).collect();
        let keep: HashSet<&Path> = visible.iter().map(|r| r.entry.path.as_path()).collect();
        self.labels.retain(|path, _| keep.contains(path.as_path()));
        for row in visible {
            let marker = match (row.entry.is_dir, row.expanded) {
                (true, true) => "▾ ",
                (true, false) => "▸ ",
                (false, _) => "  ",
            };
            self.labels.entry(row.entry.path.clone()).or_default().set(
                text,
                &format!("{marker}{}", row.entry.name),
                &style.family,
                style.metrics(),
            );
        }
    }

    fn draw<'a>(&'a self, layer: &mut Layer<'a>, cx: &PanelContext<'_>) {
        let theme = cx.theme;
        layer.quads.push(Quad::new(self.bounds, theme.panel));
        let Some(tree) = &self.tree else {
            return;
        };
        let style = self.style();
        let height = self.row_height();
        let padding = self.padding();
        let indent = style.px(14.0);
        let selected = tree.selected_index();
        for (index, row) in tree.rows().iter().enumerate() {
            let top = self.bounds.y + padding + index as f32 * height - self.scroll;
            if top + height < self.bounds.y {
                continue;
            }
            if top > self.bounds.bottom() {
                break;
            }
            let line = Rect::new(self.bounds.x, top, self.bounds.width, height);
            if Some(index) == selected {
                let color = if cx.focused {
                    theme.list_selection
                } else {
                    theme.current_line
                };
                layer
                    .quads
                    .push(Quad::new(line.intersect(&self.bounds), color));
            }
            let Some(label) = self.labels.get(&row.entry.path) else {
                continue;
            };
            let color = if row.entry.ignored {
                theme.muted
            } else {
                theme.foreground
            };
            let left = self.bounds.x + padding + row.depth as f32 * indent;
            let text_top = top + ((height - style.line_height) / 2.0).round();
            layer
                .texts
                .extend(label.item(left, text_top, self.bounds, color));
        }
    }

    fn scroll_by(&mut self, _dx: f32, dy: f32, _smooth: bool, _now: Instant) {
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll());
    }

    fn pointer_at(&self, _x: f32, _y: f32) -> Pointer {
        Pointer::Default
    }

    fn takes_keyboard(&self) -> bool {
        true
    }

    fn key(&mut self, combo: &KeyCombo) -> ViewAction {
        let page = self.page_rows() as isize;
        let Some(tree) = &mut self.tree else {
            return ViewAction::Ignored;
        };
        let named = |key| Key::Named(key);
        let action = if is(combo, named(NamedKey::Up), false) {
            tree.move_selection(-1);
            ViewAction::Handled
        } else if is(combo, named(NamedKey::Down), false) {
            tree.move_selection(1);
            ViewAction::Handled
        } else if is(combo, named(NamedKey::PageUp), false) {
            tree.move_selection(-page);
            ViewAction::Handled
        } else if is(combo, named(NamedKey::PageDown), false) {
            tree.move_selection(page);
            ViewAction::Handled
        } else if is(combo, named(NamedKey::Home), false) {
            tree.move_selection(isize::MIN / 2);
            ViewAction::Handled
        } else if is(combo, named(NamedKey::End), false) {
            tree.move_selection(isize::MAX / 2);
            ViewAction::Handled
        } else if is(combo, named(NamedKey::Left), false) {
            tree.left();
            ViewAction::Handled
        } else if is(combo, named(NamedKey::Right), false) {
            tree.right();
            ViewAction::Handled
        } else if is(combo, named(NamedKey::Enter), false) {
            return self.activate(false);
        } else if is(combo, named(NamedKey::Space), false) {
            return self.activate(true);
        } else if is(combo, Key::F(2), false) {
            let Some(path) = tree.selected() else {
                return ViewAction::Handled;
            };
            let path = tree.display_path(path);
            return self.command_line("rename", &[path.clone(), path]);
        } else if is(combo, named(NamedKey::Delete), false)
            || (combo.key == named(NamedKey::Backspace)
                && (combo.modifiers.meta || combo.modifiers.ctrl))
        {
            let Some(path) = tree.selected() else {
                return ViewAction::Handled;
            };
            let path = tree.display_path(path);
            return self.command_line("delete", &[path]);
        } else if is(combo, Key::Char('n'), false) || is(combo, Key::Char('n'), true) {
            let folder = combo.modifiers.shift;
            let dir = tree.display_path(&tree.target_dir());
            let prefix = if dir == tree.root().to_string_lossy() {
                String::new()
            } else {
                format!("{dir}/")
            };
            let command = if folder { "new-folder" } else { "new-file" };
            // Le nom reste à taper : pas de guillemets fermés derrière le curseur.
            let prefix = if prefix.is_empty() {
                prefix
            } else {
                quote(&prefix)
            };
            return ViewAction::CommandLine(format!("explorer {command} {prefix}"));
        } else if is(combo, Key::Char('m'), false) {
            let Some(path) = tree.selected() else {
                return ViewAction::Handled;
            };
            let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
            let (path, parent) = (tree.display_path(path), tree.display_path(&parent));
            return self.command_line("move", &[path, parent]);
        } else {
            ViewAction::Ignored
        };
        if action == ViewAction::Handled {
            self.follow = true;
        }
        action
    }

    fn click(&mut self, _x: f32, y: f32, clicks: u8) -> ViewAction {
        let Some(index) = self.row_at(y) else {
            return ViewAction::Handled;
        };
        let Some(tree) = &mut self.tree else {
            return ViewAction::Ignored;
        };
        let Some(row) = tree.rows().get(index).cloned() else {
            return ViewAction::Handled;
        };
        tree.select(&row.entry.path);
        if row.entry.is_dir {
            if clicks == 1 {
                tree.toggle(&row.entry.path);
            }
            ViewAction::Handled
        } else {
            Self::open(&row.entry.path, clicks < 2)
        }
    }

    fn watched_dirs(&self) -> Vec<PathBuf> {
        self.tree
            .as_ref()
            .map(ExplorerTree::watched_dirs)
            .unwrap_or_default()
    }

    fn files_changed(&mut self, paths: &[PathBuf]) {
        if let Some(tree) = &mut self.tree {
            tree.refresh(paths);
        }
    }

    fn reveal(&mut self, path: &Path) {
        ExplorerView::reveal(self, path);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Projet : src/{main.rs, lib.rs, util/}, target/ (ignoré), README.md, .gitignore, .git/.
    fn project() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("src/util")).unwrap();
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("src/main.rs"), "").unwrap();
        fs::write(root.join("src/lib.rs"), "").unwrap();
        fs::write(root.join("src/util/mod.rs"), "").unwrap();
        fs::write(root.join("README.md"), "").unwrap();
        fs::write(root.join(".gitignore"), "target/\n*.log\n").unwrap();
        fs::write(root.join("trace.log"), "").unwrap();
        (dir, root)
    }

    fn names(tree: &ExplorerTree) -> Vec<String> {
        tree.rows()
            .iter()
            .map(|r| {
                let marker = if r.entry.ignored { "~" } else { "" };
                format!("{}{marker}{}", "  ".repeat(r.depth), r.entry.name)
            })
            .collect()
    }

    #[test]
    fn premier_niveau_seulement_et_fichiers_ignores() {
        let (_dir, root) = project();
        let tree = ExplorerTree::new(&root, IgnoredFiles::Dim);
        assert_eq!(
            names(&tree),
            ["src", "~target", ".gitignore", "README.md", "~trace.log"],
            "dossiers d'abord, .git jamais, ignorés repérés"
        );
        assert_eq!(
            tree.watched_dirs(),
            vec![root.clone()],
            "rien d'autre n'est lu"
        );
        let mut hidden = ExplorerTree::new(&root, IgnoredFiles::Hide);
        assert_eq!(names(&hidden), ["src", ".gitignore", "README.md"]);
        hidden.set_mode(IgnoredFiles::Show);
        assert_eq!(hidden.rows().len(), 5);
    }

    #[test]
    fn deplier_et_naviguer_au_clavier() {
        let (_dir, root) = project();
        let mut tree = ExplorerTree::new(&root, IgnoredFiles::Hide);
        assert_eq!(tree.selected(), Some(root.join("src").as_path()));
        tree.right();
        assert_eq!(
            names(&tree),
            [
                "src",
                "  util",
                "  lib.rs",
                "  main.rs",
                ".gitignore",
                "README.md"
            ]
        );
        tree.right();
        assert_eq!(
            tree.selected(),
            Some(root.join("src/util").as_path()),
            "premier enfant"
        );
        tree.right();
        tree.right();
        assert_eq!(
            tree.selected(),
            Some(root.join("src/util/mod.rs").as_path())
        );
        tree.left();
        assert_eq!(
            tree.selected(),
            Some(root.join("src/util").as_path()),
            "parent"
        );
        tree.left();
        assert!(!tree.rows()[1].expanded, "replié");
        tree.move_selection(100);
        assert_eq!(tree.selected(), Some(root.join("README.md").as_path()));
        tree.move_selection(-100);
        tree.left();
        assert_eq!(names(&tree), ["src", ".gitignore", "README.md"]);
        assert!(
            tree.watched_dirs().len() == 1,
            "src replié n'est plus surveillé"
        );
    }

    #[test]
    fn mise_a_jour_quand_le_disque_change() {
        let (_dir, root) = project();
        let mut tree = ExplorerTree::new(&root, IgnoredFiles::Hide);
        tree.expand(&root.join("src"));
        tree.select(&root.join("src/main.rs"));
        fs::write(root.join("src/new.rs"), "").unwrap();
        fs::remove_file(root.join("src/main.rs")).unwrap();
        tree.refresh(&[root.join("src/new.rs"), root.join("src/main.rs")]);
        assert_eq!(
            names(&tree),
            [
                "src",
                "  util",
                "  lib.rs",
                "  new.rs",
                ".gitignore",
                "README.md"
            ]
        );
        assert_eq!(
            tree.selected(),
            Some(root.join("src/new.rs").as_path()),
            "la ligne qui prend sa place"
        );
        // Dossier déplié supprimé : oublié.
        tree.expand(&root.join("src/util"));
        fs::remove_dir_all(root.join("src")).unwrap();
        tree.refresh(&[root.join("src")]);
        assert_eq!(names(&tree), [".gitignore", "README.md"]);
        assert_eq!(tree.watched_dirs(), vec![root.clone()]);
        // Un dossier jamais lu ne coûte rien.
        tree.refresh(&[root.join("ailleurs/x.rs")]);
    }

    #[test]
    fn montrer_un_fichier() {
        let (_dir, root) = project();
        let mut tree = ExplorerTree::new(&root, IgnoredFiles::Dim);
        assert!(tree.reveal(&root.join("src/util/mod.rs")));
        assert_eq!(
            tree.selected(),
            Some(root.join("src/util/mod.rs").as_path())
        );
        assert!(
            tree.rows()
                .iter()
                .any(|r| r.entry.name == "mod.rs" && r.depth == 2)
        );
        assert!(!tree.reveal(Path::new("/ailleurs/x.rs")));
        assert_eq!(tree.display_path(&root.join("src/util")), "src/util");
        assert_eq!(
            tree.target_dir(),
            root.join("src/util"),
            "dossier du fichier"
        );
    }

    #[test]
    fn actions_du_clavier() {
        let (_dir, root) = project();
        let mut view = ExplorerView::new(PanelId::new(1), ViewSettings::default(), 1.0);
        view.set_bounds(Rect::new(0.0, 0.0, 200.0, 400.0));
        view.tree = Some(ExplorerTree::new(&root, IgnoredFiles::Hide));
        let combo =
            |text: &str| KeyCombo::parse(text, cupride_core::keymap::Platform::Other).unwrap();
        assert_eq!(view.key(&combo("enter")), ViewAction::Handled, "déplie src");
        assert_eq!(view.key(&combo("down")), ViewAction::Handled);
        assert_eq!(view.key(&combo("down")), ViewAction::Handled);
        let lib = root.join("src/lib.rs");
        assert_eq!(
            view.key(&combo("enter")),
            ViewAction::Run(format!("file open {}", quote(&lib.to_string_lossy())))
        );
        assert_eq!(
            view.key(&combo("f2")),
            ViewAction::CommandLine("explorer rename src/lib.rs src/lib.rs".into())
        );
        assert_eq!(
            view.key(&combo("delete")),
            ViewAction::CommandLine("explorer delete src/lib.rs".into())
        );
        assert_eq!(
            view.key(&combo("n")),
            ViewAction::CommandLine("explorer new-file src/".into())
        );
        assert_eq!(
            view.key(&combo("shift+n")),
            ViewAction::CommandLine("explorer new-folder src/".into())
        );
        assert_eq!(
            view.key(&combo("m")),
            ViewAction::CommandLine("explorer move src/lib.rs src".into())
        );
        assert_eq!(
            view.key(&combo("ctrl+p")),
            ViewAction::Ignored,
            "raccourcis globaux"
        );
        assert_eq!(view.key(&combo("x")), ViewAction::Ignored);
        // Clic : un fichier en aperçu, un double clic le garde.
        let row = view.row_height();
        let y = view.padding() + 2.5 * row;
        assert!(matches!(view.click(10.0, y, 1), ViewAction::Run(l) if l.contains("--preview")));
        assert!(matches!(view.click(10.0, y, 2), ViewAction::Run(l) if !l.contains("--preview")));
    }
}
