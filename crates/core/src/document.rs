//! Documents : un texte, son fichier sur disque et son historique.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use std::collections::BTreeMap;

use cupride_layout::PanelId;
use cupride_text::{Edit, EditKind, History, Rope, Selections, TextBuffer, TextError, Transaction};
use thiserror::Error;

/// Marque d'ordre des octets UTF-8, conservée si le fichier la contenait.
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// Nombre d'octets examinés pour reconnaître un fichier binaire (même heuristique que Git).
const BINARY_SNIFF_LEN: usize = 8_000;

/// Empreinte d'un fichier sur le disque : date de modification et taille.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DiskStamp {
    modified: Option<std::time::SystemTime>,
    len: u64,
}

/// Empreinte d'un texte, morceau par morceau (le découpage ne change pas le résultat).
fn content_hash<'a>(chunks: impl IntoIterator<Item = &'a str>) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for chunk in chunks {
        hasher.write(chunk.as_bytes());
    }
    hasher.finish()
}

/// Empreinte actuelle du fichier `path`, ou `None` s'il n'existe pas.
fn disk_stamp(path: &Path) -> Option<DiskStamp> {
    let metadata = fs::metadata(path).ok()?;
    Some(DiskStamp {
        modified: metadata.modified().ok(),
        len: metadata.len(),
    })
}

/// Le fichier d'un document, comparé à ce qu'il était à l'ouverture ou au dernier enregistrement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskState {
    /// Inchangé (ou document sans fichier).
    Same,
    /// Modifié par un autre programme.
    Changed,
    /// Supprimé ou déplacé.
    Missing,
}

/// Fin de ligne d'un document.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum LineEnding {
    /// `\n` : Linux, macOS, et la plupart des projets.
    #[default]
    Lf,
    /// `\r\n` : Windows.
    Crlf,
}

impl LineEnding {
    /// Texte de la fin de ligne.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::Crlf => "\r\n",
        }
    }

    /// Fin de ligne de la première ligne du texte, ou `None` s'il n'a qu'une ligne.
    pub fn detect(text: &str) -> Option<Self> {
        let pos = text.find('\n')?;
        Some(if text[..pos].ends_with('\r') {
            Self::Crlf
        } else {
            Self::Lf
        })
    }
}

/// Erreur d'ouverture ou d'enregistrement d'un document.
#[derive(Debug, Error)]
pub enum DocumentError {
    /// Erreur du système de fichiers.
    #[error("{}: {source}", path.display())]
    Io {
        /// Fichier concerné.
        path: PathBuf,
        /// Erreur d'origine.
        #[source]
        source: io::Error,
    },

    /// Le fichier contient des octets nuls : c'est un fichier binaire.
    #[error("{} est un fichier binaire : il n'est pas ouvert, pour ne pas le corrompre", path.display())]
    Binary {
        /// Fichier concerné.
        path: PathBuf,
    },

    /// Le fichier n'est pas encodé en UTF-8.
    #[error(
        "{} n'est pas en UTF-8 (octet invalide à la position {offset}) : il n'est pas ouvert, pour ne pas le corrompre",
        path.display()
    )]
    NotUtf8 {
        /// Fichier concerné.
        path: PathBuf,
        /// Position du premier octet invalide.
        offset: usize,
    },

    /// Le document n'a pas encore de fichier : il faut l'enregistrer sous un chemin.
    #[error("le document n'a pas de fichier : utiliser « enregistrer sous »")]
    NoPath,
}

impl DocumentError {
    fn io(path: &Path, source: io::Error) -> Self {
        Self::Io {
            path: path.to_owned(),
            source,
        }
    }
}

/// Texte décodé d'un fichier.
struct Decoded {
    text: String,
    bom: bool,
}

/// Décode le contenu d'un fichier : refuse les fichiers binaires et ceux qui ne sont pas en UTF-8.
fn decode(path: &Path, mut bytes: Vec<u8>) -> Result<Decoded, DocumentError> {
    let bom = bytes.starts_with(UTF8_BOM);
    if bom {
        bytes.drain(..UTF8_BOM.len());
    }
    if bytes[..bytes.len().min(BINARY_SNIFF_LEN)].contains(&0) {
        return Err(DocumentError::Binary {
            path: path.to_owned(),
        });
    }
    match String::from_utf8(bytes) {
        Ok(text) => Ok(Decoded { text, bom }),
        Err(error) => Err(DocumentError::NotUtf8 {
            path: path.to_owned(),
            offset: error.utf8_error().valid_up_to() + if bom { UTF8_BOM.len() } else { 0 },
        }),
    }
}

/// Un document ouvert : texte, fichier, état modifié, fin de ligne, historique et sélections.
///
/// Le texte est conservé tel quel, fins de ligne comprises : un fichier enregistré sans
/// modification est identique à l'octet près. Les nouvelles lignes insérées utilisent la fin de
/// ligne détectée à l'ouverture.
///
/// # Plusieurs vues
///
/// Un document affiché dans plusieurs panneaux a des sélections par panneau. Celles de la **vue
/// courante** (le dernier panneau actif qui l'affiche) sont [`Document::selections`], sur
/// lesquelles agissent les commandes ; celles des autres vues sont mises de côté et décalées à
/// chaque modification, pour rester sur le même texte.
#[derive(Debug)]
pub struct Document {
    buffer: Rope,
    path: Option<PathBuf>,
    modified: bool,
    line_ending: LineEnding,
    bom: bool,
    history: History,
    selections: Selections,
    /// Empreinte du fichier à l'ouverture ou au dernier enregistrement (`None` : pas encore sur
    /// le disque).
    disk: Option<DiskStamp>,
    /// Empreinte du contenu du fichier à ce moment-là.
    disk_hash: u64,
    /// Dernier changement du disque déjà signalé, pour ne pas le répéter.
    pub(crate) disk_notice: Option<DiskState>,
    /// Indentation : tabulations ou espaces, et largeur.
    indent: crate::indent::Indent,
    /// L'indentation a été choisie à la main (`edit indentation`) : la configuration ne la
    /// change plus.
    indent_set: bool,
    /// Panneau dont les sélections sont `selections`.
    view: Option<PanelId>,
    /// Sélections des autres panneaux qui affichent le document.
    other_views: BTreeMap<PanelId, Selections>,
    revision: u64,
    saves: u64,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    /// Document vide, sans fichier.
    pub fn new() -> Self {
        Self::from_text("")
    }

    /// Document sans fichier contenant `text`.
    pub fn from_text(text: &str) -> Self {
        Self {
            buffer: Rope::from_text(text),
            path: None,
            modified: false,
            line_ending: LineEnding::detect(text).unwrap_or_default(),
            bom: false,
            history: History::new(),
            selections: Selections::cursor(0),
            disk: None,
            disk_hash: 0,
            disk_notice: None,
            indent: crate::indent::Indent::default(),
            indent_set: false,
            view: None,
            other_views: BTreeMap::new(),
            revision: 0,
            saves: 0,
        }
    }

    /// Ouvre le fichier `path`. Un fichier inexistant donne un document vide associé à ce chemin,
    /// créé au premier enregistrement.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, DocumentError> {
        let path = path.into();
        let decoded = match fs::read(&path) {
            Ok(bytes) => decode(&path, bytes)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Decoded {
                text: String::new(),
                bom: false,
            },
            Err(error) => return Err(DocumentError::io(&path, error)),
        };
        let mut document = Self::from_text(&decoded.text);
        document.bom = decoded.bom;
        document.disk = disk_stamp(&path);
        document.disk_hash = content_hash([decoded.text.as_str()]);
        document.path = Some(path);
        Ok(document)
    }

    /// Relit le fichier depuis le disque ; les modifications non enregistrées et l'historique
    /// sont perdus.
    pub fn reload(&mut self) -> Result<(), DocumentError> {
        let path = self.path.clone().ok_or(DocumentError::NoPath)?;
        let reloaded = Self::open(path)?;
        let cursor = self
            .selections
            .primary()
            .head
            .min(reloaded.buffer.len_bytes());
        let (revision, saves) = (self.revision + 1, self.saves);
        let delay = self.history.group_delay();
        let (view, other_views) = (self.view, std::mem::take(&mut self.other_views));
        let (indent, indent_set) = (self.indent, self.indent_set);
        *self = reloaded;
        self.revision = revision;
        self.saves = saves;
        self.history.set_group_delay(delay);
        self.indent = indent;
        self.indent_set = indent_set;
        self.view = view;
        for (id, selections) in other_views {
            let selections = self.clamped(selections);
            self.other_views.insert(id, selections);
        }
        // Le curseur reste à peu près au même endroit, sur une frontière de caractère.
        let mut cursor = cursor;
        while !self.buffer.is_char_boundary(cursor) {
            cursor -= 1;
        }
        self.selections = Selections::cursor(cursor);
        Ok(())
    }

    /// Enregistre le document dans son fichier.
    ///
    /// L'enregistrement est atomique : le texte est écrit dans un fichier temporaire du même
    /// dossier, synchronisé sur le disque, puis renommé à la place de l'original. Un plantage
    /// pendant l'écriture laisse toujours l'ancien fichier intact. Les permissions du fichier
    /// existant sont conservées.
    pub fn save(&mut self) -> Result<(), DocumentError> {
        let path = self.path.clone().ok_or(DocumentError::NoPath)?;
        self.write_atomic(&path)?;
        self.modified = false;
        self.saves += 1;
        self.written();
        Ok(())
    }

    /// Le texte vient d'être écrit : il est la nouvelle référence du disque.
    fn written(&mut self) {
        self.disk = self.path.as_deref().and_then(disk_stamp);
        self.disk_hash = content_hash(self.buffer.chunks());
        self.disk_notice = None;
    }

    /// Enregistre le document sous `path`, qui devient son fichier.
    pub fn save_as(&mut self, path: impl Into<PathBuf>) -> Result<(), DocumentError> {
        let path = path.into();
        self.write_atomic(&path)?;
        self.path = Some(path);
        self.modified = false;
        self.saves += 1;
        self.written();
        Ok(())
    }

    fn write_atomic(&self, path: &Path) -> Result<(), DocumentError> {
        let dir = match path.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => dir,
            _ => Path::new("."),
        };
        let mut temp = tempfile::Builder::new()
            .prefix(".cupride-")
            .suffix(".tmp")
            .tempfile_in(dir)
            .map_err(|e| DocumentError::io(path, e))?;
        let write = |file: &mut fs::File| -> io::Result<()> {
            let mut writer = io::BufWriter::new(file);
            if self.bom {
                writer.write_all(UTF8_BOM)?;
            }
            for chunk in self.buffer.chunks() {
                writer.write_all(chunk.as_bytes())?;
            }
            writer.flush()?;
            writer.get_ref().sync_all()
        };
        write(temp.as_file_mut()).map_err(|e| DocumentError::io(path, e))?;
        if let Ok(metadata) = fs::metadata(path) {
            fs::set_permissions(temp.path(), metadata.permissions())
                .map_err(|e| DocumentError::io(path, e))?;
        }
        temp.persist(path)
            .map_err(|e| DocumentError::io(path, e.error))?;
        Ok(())
    }

    /// Le fichier a-t-il changé sur le disque depuis l'ouverture ou le dernier enregistrement ?
    /// Une date changée sans changement de contenu (`touch`) n'est pas un changement.
    pub fn disk_state(&self) -> DiskState {
        let Some(path) = &self.path else {
            return DiskState::Same;
        };
        let now = disk_stamp(path);
        match (self.disk, now) {
            (Some(_), None) => DiskState::Missing,
            (None, None) => DiskState::Same,
            (before, after) if before == after => DiskState::Same,
            // Date ou taille changée : le contenu tranche (un `touch` ne change rien).
            _ => match fs::read(path).ok().and_then(|b| decode(path, b).ok()) {
                Some(decoded) if content_hash([decoded.text.as_str()]) == self.disk_hash => {
                    DiskState::Same
                }
                _ => DiskState::Changed,
            },
        }
    }

    /// Prend l'état actuel du fichier comme référence : ses changements ne sont plus signalés.
    pub fn accept_disk(&mut self) {
        self.disk = self.path.as_deref().and_then(disk_stamp);
        self.disk_hash = self.disk_text().map_or(0, |t| content_hash([t.as_str()]));
        self.disk_notice = None;
    }

    /// Texte du fichier sur le disque, s'il est lisible.
    pub fn disk_text(&self) -> Option<String> {
        let path = self.path.as_deref()?;
        let bytes = fs::read(path).ok()?;
        decode(path, bytes).ok().map(|d| d.text)
    }

    /// Texte du document.
    pub fn buffer(&self) -> &Rope {
        &self.buffer
    }

    /// Fichier du document, s'il en a un.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Change le fichier du document, sans rien écrire : le fichier a été renommé ou déplacé.
    pub(crate) fn set_path(&mut self, path: PathBuf) {
        self.path = Some(path);
    }

    /// Nom affiché : nom du fichier, ou « sans titre ».
    pub fn display_name(&self) -> String {
        self.path.as_deref().and_then(Path::file_name).map_or_else(
            || "sans titre".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        )
    }

    /// Indique si le document a des modifications non enregistrées.
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// Fin de ligne utilisée pour les nouvelles lignes.
    pub fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    /// Change la fin de ligne utilisée pour les nouvelles lignes (le texte existant est inchangé).
    pub fn set_line_ending(&mut self, line_ending: LineEnding) {
        self.line_ending = line_ending;
    }

    /// Indique si le fichier commence par une marque d'ordre des octets, conservée à
    /// l'enregistrement.
    pub fn has_bom(&self) -> bool {
        self.bom
    }

    /// Indentation du document.
    pub fn indent(&self) -> crate::indent::Indent {
        self.indent
    }

    /// Change l'indentation ; avec `manual`, la configuration ne la changera plus.
    pub fn set_indent(&mut self, indent: crate::indent::Indent, manual: bool) {
        if manual || !self.indent_set {
            self.indent = indent;
            self.indent_set |= manual;
        }
    }

    /// Sélections du document.
    pub fn selections(&self) -> &Selections {
        &self.selections
    }

    /// Remplace les sélections. Elles sont ramenées dans le texte et sur des frontières de
    /// caractère.
    pub fn set_selections(&mut self, selections: Selections) {
        self.selections = self.clamped(selections);
    }

    /// Sélections ramenées dans le texte et sur des frontières de caractère.
    fn clamped(&self, mut selections: Selections) -> Selections {
        let len = self.buffer.len_bytes();
        let buffer = &self.buffer;
        let clamp = |offset: usize| {
            let mut offset = offset.min(len);
            while !buffer.is_char_boundary(offset) {
                offset -= 1;
            }
            offset
        };
        selections.transform(|s| cupride_text::Selection::new(clamp(s.anchor), clamp(s.head)));
        selections
    }

    /// Sélections affichées dans le panneau `view` : celles de la vue courante, ou celles mises
    /// de côté pour ce panneau. Un panneau inconnu du document voit les sélections courantes.
    pub fn selections_in(&self, view: PanelId) -> &Selections {
        if self.view == Some(view) {
            return &self.selections;
        }
        self.other_views.get(&view).unwrap_or(&self.selections)
    }

    /// Panneau dont les sélections sont les sélections courantes.
    pub fn current_view(&self) -> Option<PanelId> {
        self.view
    }

    /// Fait du panneau `view` la vue courante : les sélections courantes sont mises de côté pour
    /// l'ancienne vue, et celles de `view` les remplacent. Un nouveau panneau reprend les
    /// sélections courantes.
    pub fn focus_view(&mut self, view: PanelId) {
        if self.view == Some(view) {
            return;
        }
        let restored = self.other_views.remove(&view);
        if let Some(old) = self.view {
            self.other_views.insert(old, self.selections.clone());
        }
        if let Some(selections) = restored {
            self.selections = selections;
        }
        self.view = Some(view);
    }

    /// Oublie le panneau `view`, qui n'affiche plus le document.
    pub fn remove_view(&mut self, view: PanelId) {
        if self.view == Some(view) {
            self.view = None;
        } else {
            self.other_views.remove(&view);
        }
    }

    /// Décale les sélections des autres vues après les modifications `edits`.
    fn map_other_views(&mut self, edits: &[Edit]) {
        for selections in self.other_views.values_mut() {
            selections.map_all(edits);
        }
    }

    /// Numéro de révision du texte : augmente à chaque changement du texte (modification,
    /// annulation, rétablissement, rechargement).
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Nombre d'enregistrements depuis l'ouverture.
    pub fn save_count(&self) -> u64 {
        self.saves
    }

    /// Historique du document.
    pub fn history(&self) -> &History {
        &self.history
    }

    /// Historique du document, modifiable (par exemple pour régler le délai de regroupement).
    pub fn history_mut(&mut self) -> &mut History {
        &mut self.history
    }

    /// Modifie le document dans une transaction : `f` applique ses modifications au texte en
    /// partant des sélections actuelles, puis la transaction est enregistrée dans l'historique et
    /// les sélections sont mises à jour. Renvoie `true` si le texte a changé.
    ///
    /// En cas d'erreur, les modifications déjà appliquées sont annulées.
    pub fn transact(
        &mut self,
        kind: EditKind,
        f: impl FnOnce(&mut Rope, &mut Transaction) -> Result<(), TextError>,
    ) -> Result<bool, TextError> {
        let mut transaction = Transaction::with_kind(kind, self.selections.clone());
        if let Err(error) = f(&mut self.buffer, &mut transaction) {
            for edit in transaction.edits().iter().rev() {
                edit.inverse().apply(&mut self.buffer)?;
            }
            return Err(error);
        }
        let changed = !transaction.is_empty();
        self.selections = transaction.selections_after().clone();
        self.map_other_views(transaction.edits());
        if changed {
            self.modified = true;
            self.revision += 1;
            self.history.commit(transaction);
        }
        Ok(changed)
    }

    /// Annule la dernière transaction et restaure ses sélections. Renvoie `false` s'il n'y avait
    /// rien à annuler.
    pub fn undo(&mut self) -> Result<bool, TextError> {
        let edits: Vec<Edit> = self
            .history
            .peek_undo()
            .map(|t| t.edits().iter().rev().map(Edit::inverse).collect())
            .unwrap_or_default();
        let Some(selections) = self.history.undo(&mut self.buffer)? else {
            return Ok(false);
        };
        self.selections = selections;
        self.map_other_views(&edits);
        self.modified = true;
        self.revision += 1;
        Ok(true)
    }

    /// Rétablit la dernière transaction annulée. Renvoie `false` s'il n'y avait rien à rétablir.
    pub fn redo(&mut self) -> Result<bool, TextError> {
        let edits: Vec<Edit> = self
            .history
            .peek_redo()
            .map(|t| t.edits().to_vec())
            .unwrap_or_default();
        let Some(selections) = self.history.redo(&mut self.buffer)? else {
            return Ok(false);
        };
        self.selections = selections;
        self.map_other_views(&edits);
        self.modified = true;
        self.revision += 1;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cupride_text::Edit;

    fn type_at_cursor(doc: &mut Document, text: &str) {
        doc.transact(EditKind::Insert, |buffer, tx| {
            let at = tx.selections_after().primary().head;
            tx.apply(buffer, Edit::insert(at, text))
        })
        .unwrap();
    }

    #[test]
    fn ouvrir_modifier_enregistrer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.rs");
        fs::write(&path, "fn main() {}\n").unwrap();

        let mut doc = Document::open(&path).unwrap();
        assert_eq!(doc.buffer().to_text(), "fn main() {}\n");
        assert!(!doc.is_modified());
        assert_eq!(doc.display_name(), "main.rs");

        type_at_cursor(&mut doc, "// début\n");
        assert!(doc.is_modified());
        doc.save().unwrap();
        assert!(!doc.is_modified());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "// début\nfn main() {}\n"
        );
    }

    #[test]
    fn fins_de_ligne_conservees() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("windows.txt");
        fs::write(&path, "un\r\ndeux\r\n").unwrap();
        let mut doc = Document::open(&path).unwrap();
        assert_eq!(doc.line_ending(), LineEnding::Crlf);
        doc.save().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"un\r\ndeux\r\n");

        assert_eq!(LineEnding::detect("a\nb\r\n"), Some(LineEnding::Lf));
        assert_eq!(LineEnding::detect("sans fin de ligne"), None);
    }

    #[test]
    fn marque_d_ordre_des_octets_conservee() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bom.txt");
        fs::write(&path, b"\xEF\xBB\xBFtexte").unwrap();
        let mut doc = Document::open(&path).unwrap();
        assert!(doc.has_bom());
        assert_eq!(doc.buffer().to_text(), "texte");
        doc.save().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"\xEF\xBB\xBFtexte");
    }

    #[test]
    fn fichier_binaire_refuse() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("image.png");
        fs::write(&path, b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").unwrap();
        let error = Document::open(&path).unwrap_err();
        assert!(matches!(error, DocumentError::Binary { .. }), "{error}");
    }

    #[test]
    fn fichier_non_utf8_refuse_sans_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("latin1.txt");
        let latin1 = b"caf\xe9 cr\xe8me";
        fs::write(&path, latin1).unwrap();
        match Document::open(&path).unwrap_err() {
            DocumentError::NotUtf8 { offset, .. } => assert_eq!(offset, 3),
            other => panic!("{other}"),
        }
        assert_eq!(fs::read(&path).unwrap(), latin1);
    }

    #[test]
    fn fichier_inexistant_cree_a_l_enregistrement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nouveau.md");
        let mut doc = Document::open(&path).unwrap();
        assert!(doc.buffer().is_empty());
        type_at_cursor(&mut doc, "# Titre");
        doc.save().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "# Titre");
    }

    #[test]
    fn enregistrer_sous() {
        let dir = tempfile::tempdir().unwrap();
        let mut doc = Document::from_text("brouillon");
        assert_eq!(doc.display_name(), "sans titre");
        assert!(matches!(doc.save(), Err(DocumentError::NoPath)));
        let path = dir.path().join("note.txt");
        doc.save_as(&path).unwrap();
        assert_eq!(doc.path(), Some(path.as_path()));
        assert_eq!(fs::read_to_string(&path).unwrap(), "brouillon");
    }

    #[test]
    fn enregistrement_atomique_sans_fichier_temporaire_restant() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        fs::write(&path, "ancien").unwrap();
        let mut doc = Document::open(&path).unwrap();
        type_at_cursor(&mut doc, "nouveau ");
        doc.save().unwrap();
        let entries: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(entries, vec![std::ffi::OsString::from("a.txt")]);
        assert_eq!(fs::read_to_string(&path).unwrap(), "nouveau ancien");
    }

    #[test]
    fn echec_d_ecriture_laisse_l_original_intact() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("absent").join("a.txt");
        let mut doc = Document::from_text("texte");
        // Le dossier n'existe pas : l'écriture échoue sans rien créer.
        assert!(matches!(doc.save_as(&path), Err(DocumentError::Io { .. })));
        assert!(doc.path().is_none());
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn permissions_conservees() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("script.sh");
        fs::write(&path, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        let mut doc = Document::open(&path).unwrap();
        type_at_cursor(&mut doc, "# x\n");
        doc.save().unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
    }

    #[test]
    fn rechargement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        fs::write(&path, "version 1").unwrap();
        let mut doc = Document::open(&path).unwrap();
        type_at_cursor(&mut doc, "modif ");
        fs::write(&path, "version 2 sur le disque").unwrap();
        doc.reload().unwrap();
        assert_eq!(doc.buffer().to_text(), "version 2 sur le disque");
        assert!(!doc.is_modified());
        assert!(!doc.history().can_undo());
    }

    #[test]
    fn annuler_restaure_texte_et_selections() {
        let mut doc = Document::from_text("abc");
        doc.set_selections(Selections::cursor(3));
        type_at_cursor(&mut doc, "d");
        assert_eq!(doc.selections(), &Selections::cursor(4));
        assert!(doc.undo().unwrap());
        assert_eq!(doc.buffer().to_text(), "abc");
        assert_eq!(doc.selections(), &Selections::cursor(3));
        assert!(doc.redo().unwrap());
        assert_eq!(doc.buffer().to_text(), "abcd");
        assert!(!doc.redo().unwrap());
    }

    #[test]
    fn selections_par_vue() {
        let (a, b) = (PanelId::new(1), PanelId::new(2));
        let mut doc = Document::from_text("un deux trois");
        doc.focus_view(a);
        doc.set_selections(Selections::cursor(13));
        // Une nouvelle vue reprend les sélections courantes, puis s'en sépare.
        doc.focus_view(b);
        assert_eq!(doc.selections(), &Selections::cursor(13));
        doc.set_selections(Selections::cursor(0));
        assert_eq!(doc.selections_in(a), &Selections::cursor(13));
        assert_eq!(doc.selections_in(b), &Selections::cursor(0));
        // Une frappe dans b décale le curseur de a.
        type_at_cursor(&mut doc, ">> ");
        assert_eq!(doc.selections_in(a), &Selections::cursor(16));
        assert_eq!(doc.selections_in(b), &Selections::cursor(3));
        doc.undo().unwrap();
        assert_eq!(
            doc.selections_in(a),
            &Selections::cursor(13),
            "annuler aussi"
        );
        doc.redo().unwrap();
        assert_eq!(
            doc.selections_in(a),
            &Selections::cursor(16),
            "rétablir aussi"
        );
        // Retour à a : ses sélections reviennent.
        doc.focus_view(a);
        assert_eq!(doc.selections(), &Selections::cursor(16));
        assert_eq!(doc.selections_in(b), &Selections::cursor(3));
        doc.remove_view(b);
        assert_eq!(doc.selections_in(b), &Selections::cursor(16), "vue oubliée");
        assert_eq!(doc.current_view(), Some(a));
    }

    #[test]
    fn transaction_en_erreur_annulee() {
        let mut doc = Document::from_text("abc");
        let result = doc.transact(EditKind::Other, |buffer, tx| {
            tx.apply(buffer, Edit::insert(0, "x"))?;
            tx.apply(buffer, Edit::insert(99, "y"))
        });
        assert!(result.is_err());
        assert_eq!(doc.buffer().to_text(), "abc");
        assert!(!doc.is_modified());
        assert!(!doc.history().can_undo());
    }

    #[test]
    fn changements_sur_le_disque() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        fs::write(&path, "un").unwrap();
        let mut doc = Document::open(&path).unwrap();
        assert_eq!(doc.disk_state(), DiskState::Same);
        // Même contenu réécrit (date changée) : rien.
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(&path, "un").unwrap();
        assert_eq!(doc.disk_state(), DiskState::Same);
        type_at_cursor(&mut doc, "x");
        assert_eq!(doc.disk_state(), DiskState::Same, "modifié ici seulement");
        fs::write(&path, "deux, ailleurs").unwrap();
        assert_eq!(doc.disk_state(), DiskState::Changed);
        assert_eq!(doc.disk_text().as_deref(), Some("deux, ailleurs"));
        doc.accept_disk();
        assert_eq!(doc.disk_state(), DiskState::Same, "garder sa version");
        doc.save().unwrap();
        assert_eq!(
            doc.disk_state(),
            DiskState::Same,
            "son propre enregistrement"
        );
        fs::remove_file(&path).unwrap();
        assert_eq!(doc.disk_state(), DiskState::Missing);
        assert_eq!(Document::new().disk_state(), DiskState::Same);
    }
}
