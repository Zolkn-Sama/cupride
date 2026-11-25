//! Session : l'espace de travail sauvegardé à la fermeture et restauré au lancement.
//!
//! La session contient le layout (zones, onglets, proportions), et pour chaque panneau éditeur le
//! fichier affiché, ses sélections et son défilement. Elle est enregistrée en JSON dans le dossier
//! de données de l'OS, une par projet (`sessions/<projet>-<empreinte>.json`, ou `default.json`
//! sans projet). Les documents sans fichier ne sont pas gardés.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use cupride_layout::{PanelId, Workspace};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::editor::Editor;
use crate::text::{Selection, Selections};

/// Version du format : une session d'un format plus récent est ignorée.
pub const SESSION_VERSION: u32 = 1;

/// Espace de travail enregistré.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// Version du format.
    pub version: u32,
    /// Panneaux, layout et panneau actif ; l'état de chaque panneau éditeur est un
    /// [`EditorState`].
    pub workspace: Workspace,
}

/// Défilement d'une vue d'éditeur : première ligne visible (fractionnaire) et décalage horizontal,
/// en pixels logiques (indépendant de l'écran).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ScrollPosition {
    /// Ligne en haut de la vue.
    pub line: f32,
    /// Défilement horizontal.
    pub x: f32,
}

/// État enregistré d'un panneau éditeur.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditorState {
    /// Fichier affiché.
    pub path: PathBuf,
    /// Sélections : (ancre, tête), en octets.
    #[serde(default)]
    pub selections: Vec<(usize, usize)>,
    /// Rang de la sélection principale.
    #[serde(default)]
    pub primary: usize,
    /// Défilement de la vue.
    #[serde(default)]
    pub scroll: Option<ScrollPosition>,
}

impl EditorState {
    fn selections(&self) -> Option<Selections> {
        let ranges = self
            .selections
            .iter()
            .map(|&(anchor, head)| Selection::new(anchor, head))
            .collect();
        Selections::from_vec(ranges, self.primary)
    }
}

/// Erreur de lecture ou d'écriture d'une session.
#[derive(Debug, Error)]
pub enum SessionError {
    /// Erreur du système de fichiers.
    #[error("session {}: {source}", path.display())]
    Io {
        /// Fichier concerné.
        path: PathBuf,
        /// Erreur d'origine.
        #[source]
        source: io::Error,
    },
    /// Contenu illisible.
    #[error("session {} illisible : {source}", path.display())]
    Format {
        /// Fichier concerné.
        path: PathBuf,
        /// Erreur d'origine.
        #[source]
        source: serde_json::Error,
    },
    /// Format plus récent que celui de cette version de CuprIDE.
    #[error("session {} d'une version plus récente ({version}) : ignorée", path.display())]
    Version {
        /// Fichier concerné.
        path: PathBuf,
        /// Version trouvée.
        version: u32,
    },
}

/// Dossier des sessions, dans le dossier de données de l'OS.
pub fn sessions_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "cupride").map(|d| d.data_dir().join("sessions"))
}

/// Nom du fichier de session d'un projet : son nom de dossier et une empreinte de son chemin
/// complet (deux projets de même nom ont deux sessions) ; `default.json` sans projet.
pub fn session_file_name(project: Option<&Path>) -> String {
    let Some(project) = project else {
        return "default.json".to_owned();
    };
    let name: String = project
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!(
        "{name}-{:016x}.json",
        fnv1a(project.to_string_lossy().as_bytes())
    )
}

/// Empreinte FNV-1a 64 bits : stable d'une version de Rust à l'autre, contrairement au hachage
/// de la bibliothèque standard.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Lit une session. Un fichier absent donne `None`.
pub fn load(path: &Path) -> Result<Option<Session>, SessionError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(SessionError::Io {
                path: path.to_owned(),
                source,
            });
        }
    };
    let session: Session = serde_json::from_str(&text).map_err(|source| SessionError::Format {
        path: path.to_owned(),
        source,
    })?;
    if session.version > SESSION_VERSION {
        return Err(SessionError::Version {
            path: path.to_owned(),
            version: session.version,
        });
    }
    Ok(Some(session))
}

/// Enregistre une session, de façon atomique (fichier temporaire puis renommage) ; le dossier est
/// créé au besoin.
pub fn save(session: &Session, path: &Path) -> Result<(), SessionError> {
    let io = |source| SessionError::Io {
        path: path.to_owned(),
        source,
    };
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir).map_err(io)?;
    let json = serde_json::to_vec_pretty(session).map_err(|source| SessionError::Format {
        path: path.to_owned(),
        source,
    })?;
    let mut temp = tempfile::NamedTempFile::new_in(dir).map_err(io)?;
    temp.write_all(&json).map_err(io)?;
    temp.as_file().sync_all().map_err(io)?;
    temp.persist(path).map_err(|e| io(e.error))?;
    Ok(())
}

impl Editor {
    /// Défilement à donner à la vue du panneau `panel` à sa création (session restaurée).
    pub fn panel_scroll(&self, panel: PanelId) -> Option<ScrollPosition> {
        self.panel_scrolls.get(&panel).copied()
    }

    /// Mémorise le défilement de la vue du panneau `panel`, pour la session.
    pub fn set_panel_scroll(&mut self, panel: PanelId, scroll: ScrollPosition) {
        self.panel_scrolls.insert(panel, scroll);
    }

    /// Session de l'espace de travail actuel. Les panneaux des documents sans fichier n'y sont
    /// pas.
    pub fn capture_session(&self) -> Session {
        let mut workspace = self.workspace().clone();
        let panels: Vec<PanelId> = self.workspace().panels().map(|p| p.id()).collect();
        for panel in panels {
            let Some(id) = self.panel_document(panel) else {
                continue;
            };
            let state = self.document(id).and_then(|document| {
                let selections = document.selections_in(panel);
                Some(EditorState {
                    path: document.path()?.to_owned(),
                    selections: selections.iter().map(|s| (s.anchor, s.head)).collect(),
                    primary: selections.primary_index(),
                    scroll: self.panel_scroll(panel),
                })
            });
            match (state, workspace.panel_mut(panel)) {
                (Some(state), Some(model)) => model.store_state(&state),
                _ => {
                    workspace.remove(panel);
                }
            }
        }
        Session {
            version: SESSION_VERSION,
            workspace,
        }
    }

    /// Remplace l'espace de travail par celui de `session`. Les fichiers sont rouverts (un
    /// fichier affiché dans plusieurs panneaux n'est ouvert qu'une fois), avec leurs sélections
    /// et leur défilement. Un fichier supprimé ou illisible entre-temps : son panneau est fermé
    /// et le problème renvoyé, pour être signalé. Les documents déjà ouverts restent ouverts,
    /// même s'ils ne sont plus affichés.
    pub fn restore_session(&mut self, session: Session) -> Vec<String> {
        let mut workspace = session.workspace;
        let mut problems = Vec::new();
        let mut bindings = Vec::new();
        let panels: Vec<PanelId> = workspace.panels().map(|p| p.id()).collect();
        for panel in panels {
            let Some(model) = workspace.panel(panel) else {
                continue;
            };
            if !model.kind().is_editor() {
                continue;
            }
            let Some(state) = model.state_as::<EditorState>() else {
                workspace.remove(panel);
                continue;
            };
            if !state.path.exists() {
                let name = state.path.file_name().map_or_else(
                    || state.path.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                problems.push(format!(
                    "« {name} » n'existe plus ({}) : son panneau est fermé",
                    state.path.display()
                ));
                workspace.remove(panel);
                continue;
            }
            match self.load(&state.path) {
                Ok(document) => bindings.push((panel, document, state)),
                Err(error) => {
                    problems.push(format!("{error} : son panneau est fermé"));
                    workspace.remove(panel);
                }
            }
        }
        // Les panneaux d'avant disparaissent.
        for (panel, document) in std::mem::take(&mut self.panel_documents) {
            if let Some(document) = self.document_mut(document) {
                document.remove_view(panel);
            }
        }
        self.panel_scrolls.clear();
        let (renamed, _) = self.workspace_mut().replace(workspace);
        for (old, document, state) in bindings {
            let Some(&panel) = renamed.get(&old) else {
                continue;
            };
            self.panel_documents.insert(panel, document);
            if let Some(scroll) = state.scroll {
                self.panel_scrolls.insert(panel, scroll);
            }
            if let Some(doc) = self.document_mut(document) {
                doc.focus_view(panel);
                if let Some(selections) = state.selections() {
                    doc.set_selections(selections);
                }
            }
        }
        let active = self
            .active_panel()
            .or_else(|| self.workspace().tree().panels().first().copied());
        match active {
            Some(panel) => {
                self.focus_panel(panel);
            }
            None => {
                self.sync_active_document();
                self.flush_events();
            }
        }
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewPanel;
    use crate::layout::{Direction, PanelKind};

    #[test]
    fn noms_des_fichiers_de_session() {
        assert_eq!(session_file_name(None), "default.json");
        let a = session_file_name(Some(Path::new("/home/moi/mon projet")));
        let b = session_file_name(Some(Path::new("/tmp/mon projet")));
        assert!(a.starts_with("mon_projet-"), "{a}");
        assert_ne!(a, b, "même nom, chemins différents");
        assert_eq!(
            a,
            session_file_name(Some(Path::new("/home/moi/mon projet")))
        );
    }

    #[test]
    fn enregistrer_puis_restaurer() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.rs"), dir.path().join("b.rs"));
        fs::write(&a, "fn a() {}\n").unwrap();
        fs::write(&b, "fn b() {}\n// fin\n").unwrap();

        let mut editor = Editor::new();
        editor.open(&a).unwrap();
        editor
            .active_document_mut()
            .unwrap()
            .set_selections(Selections::single(Selection::new(3, 4)));
        let doc_b = editor.open(&b).unwrap();
        let right = editor
            .open_editor_panel(doc_b, NewPanel::Split(Direction::Right))
            .unwrap();
        editor
            .active_document_mut()
            .unwrap()
            .set_selections(Selections::cursor(12));
        editor.set_panel_scroll(right, ScrollPosition { line: 1.5, x: 0.0 });
        editor.open_panel(
            PanelKind::new(PanelKind::EXPLORER),
            NewPanel::Split(Direction::Left),
        );
        editor.new_document(); // sans fichier : pas dans la session
        editor.focus_panel(right);

        let path = dir.path().join("sessions").join("s.json");
        save(&editor.capture_session(), &path).unwrap();
        let session = load(&path).unwrap().unwrap();

        let mut restored = Editor::new();
        let problems = restored.restore_session(session);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(restored.document_count(), 2, "b ouvert une seule fois");
        assert_eq!(
            restored.workspace().tree().groups().len(),
            editor.workspace().tree().groups().len()
        );
        assert_eq!(
            restored.workspace().panel_count(),
            4,
            "sans le document vide"
        );
        let active = restored.active_panel().unwrap();
        assert_eq!(restored.active_document().unwrap().display_name(), "b.rs");
        assert_eq!(
            restored.active_document().unwrap().selections(),
            &Selections::cursor(12)
        );
        assert_eq!(
            restored.panel_scroll(active),
            Some(ScrollPosition { line: 1.5, x: 0.0 })
        );
        // a | explorateur | b, l'explorateur seul dans sa zone (le document vide est parti).
        let groups = restored.workspace().tree().groups();
        assert_eq!(groups[0].len(), 2, "a et b en onglets");
        assert_eq!(groups[1].len(), 1);
        let explorer = groups[1].panels()[0];
        assert_eq!(
            restored
                .workspace()
                .panel(explorer)
                .unwrap()
                .kind()
                .as_str(),
            "explorer"
        );
        assert!(restored.workspace().is_valid());
    }

    #[test]
    fn fichier_supprime_panneau_ferme() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.rs"), dir.path().join("b.rs"));
        fs::write(&a, "a").unwrap();
        fs::write(&b, "b").unwrap();
        let mut editor = Editor::new();
        editor.open(&a).unwrap();
        editor.execute("split right").unwrap();
        editor.open(&b).unwrap();
        let session = editor.capture_session();
        fs::remove_file(&b).unwrap();

        let mut restored = Editor::new();
        let problems = restored.restore_session(session);
        assert_eq!(problems.len(), 1);
        assert!(
            problems[0].contains("« b.rs » n'existe plus"),
            "{problems:?}"
        );
        assert_eq!(restored.document_count(), 1);
        assert_eq!(
            restored.workspace().panel_count(),
            2,
            "a reste dans ses deux zones"
        );
        assert!(restored.workspace().is_valid());
    }

    #[test]
    fn session_absente_illisible_ou_trop_recente() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(&dir.path().join("absente.json")).unwrap().is_none());
        let path = dir.path().join("abimee.json");
        fs::write(&path, "{ pas du json").unwrap();
        assert!(matches!(load(&path), Err(SessionError::Format { .. })));
        let mut session = Editor::new().capture_session();
        session.version = SESSION_VERSION + 1;
        save(&session, &path).unwrap();
        assert!(matches!(load(&path), Err(SessionError::Version { .. })));
    }
}
