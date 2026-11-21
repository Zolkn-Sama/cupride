//! État global de l'éditeur.

use std::io;
use std::path::{Path, PathBuf};

use cupride_layout::Workspace;
use slotmap::{SecondaryMap, SlotMap};

use crate::clipboard::{Clipboard, LastCopy, MemoryClipboard};
use crate::command::{
    CommandError, CommandOutput, CommandRegistry, CommandResult, builtin, parser,
};
use crate::config::{self, Config};
use crate::document::{Document, DocumentError};
use crate::events::{Event, EventBus};
use crate::keymap::{Keymap, KeymapIssue, Platform, default_bindings};

slotmap::new_key_type! {
    /// Identifiant stable d'un document ouvert. Il reste valide tant que le document est ouvert,
    /// et n'est jamais réutilisé pour un autre document.
    pub struct DocumentId;
}

/// Crans de zoom de l'interface, de 50 % à 300 %.
pub(crate) const ZOOM_STEPS: [f32; 13] = [
    0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0,
];

/// Lignes d'une page tant que l'interface n'en a pas fourni le nombre.
const DEFAULT_PAGE_LINES: usize = 30;

/// État complet de l'éditeur, sans aucune dépendance graphique.
///
/// Tout ce que l'interface affiche et tout ce que les commandes modifient vit ici : documents
/// ouverts, document actif, workspace (fenêtres et layout), registre des commandes. Un fichier
/// ouvert plusieurs fois correspond toujours à un seul document en mémoire.
#[derive(Debug)]
pub struct Editor {
    documents: SlotMap<DocumentId, Document>,
    active: Option<DocumentId>,
    workspace: Workspace,
    commands: CommandRegistry,
    keymap: Keymap,
    config: Config,
    config_path: Option<PathBuf>,
    events: EventBus,
    /// État déjà publié : (révision, enregistrements) de chaque document, et document actif.
    published: SecondaryMap<DocumentId, (u64, u64)>,
    published_active: Option<DocumentId>,
    /// Nombre de lignes d'une page, pour `edit move page-up` : fourni par la vue.
    page_lines: usize,
    clipboard: Box<dyn Clipboard>,
    /// Dernière copie faite dans l'IDE, pour redistribuer ses fragments entre les curseurs.
    last_copy: Option<LastCopy>,
    /// Zoom de l'interface (1 : taille normale), multiplié par l'échelle de l'écran.
    zoom: f32,
    /// Dossier ouvert comme projet, s'il y en a un.
    project: Option<PathBuf>,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

impl Editor {
    /// Éditeur sans document, avec les commandes intégrées.
    pub fn new() -> Self {
        let mut editor = Self::without_builtins();
        builtin::register_all(&mut editor.commands)
            .expect("les commandes intégrées ont des noms uniques");
        editor.keymap = Keymap::with_defaults(Platform::current());
        editor.config_path = config::default_path();
        editor
    }

    /// Éditeur sans document, sans commande et sans raccourci : pour les tests, ou pour composer
    /// son propre jeu de commandes.
    pub fn without_builtins() -> Self {
        Self {
            documents: SlotMap::with_key(),
            active: None,
            workspace: Workspace::new(),
            commands: CommandRegistry::new(),
            keymap: Keymap::new(),
            config: Config::default(),
            config_path: None,
            events: EventBus::new(),
            published: SecondaryMap::new(),
            published_active: None,
            page_lines: DEFAULT_PAGE_LINES,
            clipboard: Box::new(MemoryClipboard::default()),
            last_copy: None,
            zoom: 1.0,
            project: None,
        }
    }

    /// Ouvre le fichier `path` et en fait le document actif. Si ce fichier est déjà ouvert (même
    /// sous un autre chemin menant au même fichier), le document existant est réutilisé.
    pub fn open(&mut self, path: impl AsRef<Path>) -> Result<DocumentId, DocumentError> {
        let path = normalize(path.as_ref()).map_err(|source| DocumentError::Io {
            path: path.as_ref().to_owned(),
            source,
        })?;
        let id = match self.find_by_path(&path) {
            Some(id) => id,
            None => {
                let mut document = Document::open(path)?;
                self.configure(&mut document);
                self.documents.insert(document)
            }
        };
        self.active = Some(id);
        self.flush_events();
        Ok(id)
    }

    /// Crée un document vide sans fichier et en fait le document actif.
    pub fn new_document(&mut self) -> DocumentId {
        let mut document = Document::new();
        document.set_line_ending(self.config.editor.line_ending.for_new_document());
        self.configure(&mut document);
        let id = self.documents.insert(document);
        self.active = Some(id);
        self.flush_events();
        id
    }

    /// Ajoute un document déjà construit et en fait le document actif.
    pub fn insert_document(&mut self, document: Document) -> DocumentId {
        let id = self.documents.insert(document);
        self.active = Some(id);
        self.flush_events();
        id
    }

    /// Document ouvert sur le fichier `path`, s'il y en a un.
    pub fn find_by_path(&self, path: &Path) -> Option<DocumentId> {
        let path = normalize(path).ok()?;
        self.documents
            .iter()
            .find(|(_, doc)| doc.path() == Some(path.as_path()))
            .map(|(id, _)| id)
    }

    /// Ferme le document `id` et le renvoie. Si c'était le document actif, le dernier document
    /// ouvert devient actif.
    pub fn close(&mut self, id: DocumentId) -> Option<Document> {
        let document = self.documents.remove(id)?;
        if self.active == Some(id) {
            self.active = self.documents.keys().last();
        }
        self.flush_events();
        Some(document)
    }

    /// Document `id`.
    pub fn document(&self, id: DocumentId) -> Option<&Document> {
        self.documents.get(id)
    }

    /// Document `id`, modifiable.
    pub fn document_mut(&mut self, id: DocumentId) -> Option<&mut Document> {
        self.documents.get_mut(id)
    }

    /// Documents ouverts.
    pub fn documents(&self) -> impl Iterator<Item = (DocumentId, &Document)> {
        self.documents.iter()
    }

    /// Nombre de documents ouverts.
    pub fn document_count(&self) -> usize {
        self.documents.len()
    }

    /// Identifiant du document actif.
    pub fn active_id(&self) -> Option<DocumentId> {
        self.active
    }

    /// Document actif.
    pub fn active_document(&self) -> Option<&Document> {
        self.documents.get(self.active?)
    }

    /// Document actif, modifiable.
    pub fn active_document_mut(&mut self) -> Option<&mut Document> {
        self.documents.get_mut(self.active?)
    }

    /// Fait de `id` le document actif. Renvoie `false` si ce document n'existe pas.
    pub fn set_active(&mut self, id: DocumentId) -> bool {
        if self.documents.contains_key(id) {
            self.active = Some(id);
            self.flush_events();
            true
        } else {
            false
        }
    }

    /// Registre des commandes.
    pub fn commands(&self) -> &CommandRegistry {
        &self.commands
    }

    /// Registre des commandes, modifiable (pour en enregistrer de nouvelles).
    pub fn commands_mut(&mut self) -> &mut CommandRegistry {
        &mut self.commands
    }

    /// Exécute une ligne de commande, telle que tapée par l'utilisateur :
    /// `file open "mon fichier.rs"`. `<commande> --help` renvoie l'aide de la commande.
    /// Une ligne vide ne fait rien.
    pub fn execute(&mut self, line: &str) -> CommandResult {
        match parser::parse(&self.commands, line)? {
            Some(parsed) => self.run(&parsed.name, &parsed.args),
            None => Ok(CommandOutput::None),
        }
    }

    /// Exécute la commande `full_name` (`file open`) avec les arguments `args`, déjà découpés.
    pub fn run(&mut self, full_name: &str, args: &[String]) -> CommandResult {
        let command = self
            .commands
            .get(full_name)
            .ok_or_else(|| CommandError::Unknown {
                name: full_name.to_owned(),
                suggestion: parser::suggest(&self.commands, full_name),
            })?;
        if args.iter().any(|a| a == "--help" || a == "-h") {
            return Ok(CommandOutput::Message(command.help_text()));
        }
        let matches = command
            .parse(args)
            .map_err(|error| parser::describe_clap_error(full_name, &error))?;
        let handler = command.handler();
        let result = handler(self, &matches);
        self.flush_events();
        result
    }

    /// Abonnements aux événements du noyau.
    pub fn events(&self) -> &EventBus {
        &self.events
    }

    /// Abonnements aux événements du noyau, modifiables (pour s'abonner).
    pub fn events_mut(&mut self) -> &mut EventBus {
        &mut self.events
    }

    /// Publie les événements survenus depuis la dernière publication : documents ouverts,
    /// modifiés, enregistrés, fermés, puis changement de document actif. Appelé automatiquement
    /// après chaque commande et chaque ouverture, fermeture ou activation.
    pub fn flush_events(&mut self) {
        let mut events = Vec::new();
        for (id, document) in &self.documents {
            let now = (document.revision(), document.save_count());
            match self.published.insert(id, now) {
                None => events.push(Event::DocumentOpened(id)),
                Some((revision, saves)) => {
                    if revision != now.0 {
                        events.push(Event::DocumentModified(id));
                    }
                    if saves != now.1 {
                        events.push(Event::DocumentSaved(id));
                    }
                }
            }
        }
        let closed: Vec<DocumentId> = self
            .published
            .keys()
            .filter(|id| !self.documents.contains_key(*id))
            .collect();
        for id in closed {
            self.published.remove(id);
            events.push(Event::DocumentClosed(id));
        }
        if self.published_active != self.active {
            events.push(Event::FocusChanged {
                from: self.published_active,
                to: self.active,
            });
            self.published_active = self.active;
        }
        for event in events {
            self.events.publish(event);
        }
    }

    /// Raccourcis clavier.
    pub fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// Raccourcis clavier, modifiables.
    pub fn keymap_mut(&mut self) -> &mut Keymap {
        &mut self.keymap
    }

    /// Configuration en vigueur.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Emplacement du fichier de configuration (par défaut, celui du dossier de configuration de
    /// l'OS).
    pub fn config_path(&self) -> Option<&Path> {
        self.config_path.as_deref()
    }

    /// Change l'emplacement du fichier de configuration.
    pub fn set_config_path(&mut self, path: Option<PathBuf>) {
        self.config_path = path;
    }

    /// Applique une configuration : raccourcis par défaut complétés par ceux de l'utilisateur,
    /// réglages appliqués aux documents ouverts. Renvoie les problèmes de raccourcis à signaler.
    pub fn apply_config(&mut self, config: Config) -> Vec<KeymapIssue> {
        let platform = Platform::current();
        let defaults = default_bindings(platform)
            .into_iter()
            .map(|(keys, command)| (keys.to_owned(), command.to_owned()));
        let user = config
            .keys
            .iter()
            .filter(|(_, command)| !command.trim().is_empty())
            .map(|(keys, command)| (keys.clone(), command.clone()));
        let (mut keymap, issues) = Keymap::from_entries(defaults.chain(user), platform);
        // Les doublons sont voulus : un raccourci de l'utilisateur remplace celui par défaut.
        let mut issues: Vec<KeymapIssue> = issues
            .into_iter()
            .filter(|issue| !matches!(issue, KeymapIssue::Duplicate { .. }))
            .collect();
        for (keys, _) in config.keys.iter().filter(|(_, c)| c.trim().is_empty()) {
            match crate::keymap::KeySequence::parse(keys, platform) {
                Ok(sequence) => {
                    keymap.unbind(&sequence);
                }
                Err(error) => issues.push(KeymapIssue::Invalid {
                    keys: keys.clone(),
                    error,
                }),
            }
        }
        self.keymap = keymap;
        self.config = config;
        let delay = self.config.editor.undo_group_delay();
        for (_, document) in &mut self.documents {
            document.history_mut().set_group_delay(delay);
        }
        issues
    }

    /// Lit le fichier de configuration et l'applique. Renvoie les messages à afficher : erreur
    /// (avec ligne et colonne), avertissements et problèmes de raccourcis. En cas d'erreur, les
    /// valeurs par défaut sont appliquées.
    pub fn reload_config(&mut self) -> Vec<String> {
        let Some(path) = self.config_path.clone() else {
            return vec!["dossier de configuration introuvable : valeurs par défaut".to_owned()];
        };
        let loaded = config::load(&path);
        let mut messages: Vec<String> = loaded.error.iter().map(ToString::to_string).collect();
        messages.extend(loaded.warnings);
        messages.extend(
            self.apply_config(loaded.config)
                .iter()
                .map(ToString::to_string),
        );
        messages
    }

    /// Applique la configuration à un document qui vient d'être créé ou ouvert.
    fn configure(&self, document: &mut Document) {
        document
            .history_mut()
            .set_group_delay(self.config.editor.undo_group_delay());
    }

    /// Dossier ouvert comme projet.
    pub fn project(&self) -> Option<&Path> {
        self.project.as_deref()
    }

    /// Ouvre le dossier `path` comme projet (chemin absolu et canonique). Erreur si ce n'est
    /// pas un dossier existant.
    pub fn set_project(&mut self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref().canonicalize()?;
        if !path.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                "ce n'est pas un dossier",
            ));
        }
        self.project = Some(path);
        Ok(())
    }

    /// Zoom de l'interface : 1 pour la taille normale.
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Change le zoom de l'interface, borné de 50 % à 300 %.
    pub fn set_zoom(&mut self, zoom: f32) {
        let (min, max) = (ZOOM_STEPS[0], ZOOM_STEPS[ZOOM_STEPS.len() - 1]);
        self.zoom = if zoom.is_finite() {
            zoom.clamp(min, max)
        } else {
            1.0
        };
    }

    /// Presse-papiers utilisé par `edit copy`, `cut` et `paste`.
    pub fn clipboard_mut(&mut self) -> &mut dyn Clipboard {
        self.clipboard.as_mut()
    }

    /// Remplace le presse-papiers : l'application y met celui de l'OS.
    pub fn set_clipboard(&mut self, clipboard: Box<dyn Clipboard>) {
        self.clipboard = clipboard;
    }

    pub(crate) fn last_copy(&self) -> Option<&LastCopy> {
        self.last_copy.as_ref()
    }

    pub(crate) fn set_last_copy(&mut self, copy: LastCopy) {
        self.last_copy = Some(copy);
    }

    /// Nombre de lignes parcourues par `edit move page-up` et `page-down`.
    pub fn page_lines(&self) -> usize {
        self.page_lines
    }

    /// Change le nombre de lignes d'une page : l'interface y met le nombre de lignes visibles.
    pub fn set_page_lines(&mut self, lines: usize) {
        self.page_lines = lines.max(1);
    }

    /// Fenêtres et layout.
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// Fenêtres et layout, modifiables.
    pub fn workspace_mut(&mut self) -> &mut Workspace {
        &mut self.workspace
    }
}

/// Chemin absolu et canonique : deux chemins menant au même fichier donnent le même résultat.
/// Un fichier qui n'existe pas encore est résolu à partir de son dossier.
pub(crate) fn normalize(path: &Path) -> io::Result<PathBuf> {
    if let Ok(canonical) = path.canonicalize() {
        return Ok(canonical);
    }
    let absolute = std::path::absolute(path)?;
    match (absolute.parent(), absolute.file_name()) {
        (Some(parent), Some(name)) => match parent.canonicalize() {
            Ok(parent) => Ok(parent.join(name)),
            Err(_) => Ok(absolute),
        },
        _ => Ok(absolute),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn meme_fichier_un_seul_document() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        let path = dir.path().join("src").join("lib.rs");
        fs::write(&path, "// lib").unwrap();

        let mut editor = Editor::new();
        let a = editor.open(&path).unwrap();
        let b = editor
            .open(
                dir.path()
                    .join("src")
                    .join(".")
                    .join("..")
                    .join("src/lib.rs"),
            )
            .unwrap();
        assert_eq!(a, b);
        assert_eq!(editor.document_count(), 1);
    }

    #[test]
    fn nouveau_fichier_ouvert_deux_fois() {
        let dir = tempfile::tempdir().unwrap();
        let mut editor = Editor::new();
        let a = editor.open(dir.path().join("nouveau.txt")).unwrap();
        let b = editor
            .open(dir.path().join(".").join("nouveau.txt"))
            .unwrap();
        assert_eq!(a, b);
    }

    #[cfg(unix)]
    #[test]
    fn lien_symbolique_meme_document() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("vrai.txt");
        let link = dir.path().join("lien.txt");
        fs::write(&real, "x").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let mut editor = Editor::new();
        assert_eq!(editor.open(&real).unwrap(), editor.open(&link).unwrap());
    }

    #[test]
    fn document_actif_et_fermeture() {
        let mut editor = Editor::new();
        assert!(editor.active_document().is_none());
        let a = editor.new_document();
        let b = editor.new_document();
        assert_eq!(editor.active_id(), Some(b));
        assert!(editor.set_active(a));
        assert_eq!(editor.active_id(), Some(a));
        assert!(editor.close(a).is_some());
        assert_eq!(editor.active_id(), Some(b));
        assert!(!editor.set_active(a), "identifiant fermé");
        assert!(editor.document(a).is_none());
        editor.close(b);
        assert_eq!(editor.active_id(), None);
        // Les identifiants ne sont jamais réutilisés.
        let c = editor.new_document();
        assert_ne!(c, a);
        assert_ne!(c, b);
    }

    #[test]
    fn erreur_d_ouverture_ne_cree_pas_de_document() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("binaire.bin");
        fs::write(&path, b"\0\0\0").unwrap();
        let mut editor = Editor::new();
        assert!(editor.open(&path).is_err());
        assert_eq!(editor.document_count(), 0);
    }
}
