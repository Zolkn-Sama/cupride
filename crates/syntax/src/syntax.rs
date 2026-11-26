//! Arbre syntaxique d'un document, tenu à jour à chaque modification.
//!
//! Chaque modification est transmise à l'arbre ([`Syntax::edit`]), puis le texte est réanalysé
//! en réutilisant l'ancien arbre : seule la zone modifiée est vraiment analysée. Une analyse
//! faite sur le thread principal a un budget de [`SYNC_BUDGET`] ; au-delà (premier passage sur un
//! gros fichier, modification d'un fichier énorme), elle est reprise dans un thread, sur une copie
//! du texte. En attendant, l'ancien arbre, décalé par les modifications, reste utilisable.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use cupride_text::{Edit, Rope, TextBuffer};
use tree_sitter::{InputEdit, Node, ParseOptions, Parser, Tree};

use crate::edits::input_edits;
use crate::languages::LanguageConfig;
use crate::source::Source;

/// Appelé depuis le thread d'analyse quand un arbre est prêt : l'application y réveille sa
/// boucle d'événements, qui appelle [`Syntax::poll`].
#[derive(Clone)]
pub struct Waker(Arc<dyn Fn() + Send + Sync>);

impl Waker {
    /// Réveil qui appelle `wake`.
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self(Arc::new(wake))
    }

    fn wake(&self) {
        (self.0)();
    }
}

impl std::fmt::Debug for Waker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Waker")
    }
}

/// Temps maximal d'une analyse sur le thread principal, au-delà duquel elle passe dans un thread.
pub const SYNC_BUDGET: Duration = Duration::from_millis(4);

/// Au-delà de cette taille, la première analyse est faite directement dans un thread.
const SYNC_INITIAL_MAX: usize = 256 * 1024;

/// Analyse en cours dans un thread.
struct Job {
    result: Receiver<Option<Tree>>,
    cancel: Arc<AtomicBool>,
    /// Modifications arrivées pendant l'analyse : appliquées à son arbre à l'arrivée.
    edits: Vec<InputEdit>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Arbre syntaxique d'un document, dans un langage.
pub struct Syntax {
    config: &'static LanguageConfig,
    parser: Parser,
    tree: Option<Tree>,
    /// L'arbre correspond au texte actuel (sinon, il a seulement été décalé par les
    /// modifications, en attendant la réanalyse).
    fresh: bool,
    job: Option<Job>,
    waker: Option<Waker>,
    /// Change à chaque changement de l'arbre : pour invalider ce qui en est déduit.
    version: u64,
    /// Durée de la dernière analyse sur le thread principal.
    last_parse: Duration,
}

impl std::fmt::Debug for Syntax {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Syntax")
            .field("language", &self.config.name)
            .field("parsed", &self.tree.is_some())
            .field("fresh", &self.fresh)
            .field("background", &self.job.is_some())
            .finish_non_exhaustive()
    }
}

impl Syntax {
    /// Analyse `text` dans le langage `config`. Avec `waker`, une analyse longue est faite dans
    /// un thread ; sans, elle est toujours faite tout de suite.
    pub fn new(config: &'static LanguageConfig, text: &Rope, waker: Option<Waker>) -> Self {
        let mut parser = Parser::new();
        if let Err(error) = parser.set_language(&config.language) {
            tracing::warn!(langage = config.name, %error, "grammaire incompatible");
        }
        let mut syntax = Self {
            config,
            parser,
            tree: None,
            fresh: false,
            job: None,
            waker,
            version: 0,
            last_parse: Duration::ZERO,
        };
        syntax.update(text);
        syntax
    }

    /// Langage analysé.
    pub fn config(&self) -> &'static LanguageConfig {
        self.config
    }

    /// Change la fonction appelée à la fin d'une analyse en arrière-plan.
    pub fn set_waker(&mut self, waker: Option<Waker>) {
        self.waker = waker;
    }

    /// Le texte vient de recevoir les modifications `applied` (dans l'ordre) : l'arbre est mis à
    /// jour, puis le texte réanalysé.
    pub fn edited(&mut self, text: &mut Rope, applied: &[Edit]) {
        if applied.is_empty() {
            return;
        }
        match input_edits(text, applied) {
            Some(edits) => self.edit(&edits),
            // Modifications trop nombreuses : analyse complète.
            None => {
                self.tree = None;
                self.job = None;
                self.fresh = false;
                self.version += 1;
            }
        }
        self.update(text);
    }

    /// Transmet des modifications à l'arbre (et à l'analyse en cours), sans réanalyser.
    pub fn edit(&mut self, edits: &[InputEdit]) {
        if let Some(tree) = &mut self.tree {
            for edit in edits {
                tree.edit(edit);
            }
        }
        if let Some(job) = &mut self.job {
            job.edits.extend_from_slice(edits);
        }
        self.fresh = false;
        self.version += 1;
    }

    /// Réanalyse tout le texte, sans réutiliser l'arbre : le texte a été remplacé.
    pub fn reset(&mut self, text: &Rope) {
        self.tree = None;
        self.job = None;
        self.fresh = false;
        self.version += 1;
        self.update(text);
    }

    /// Réanalyse le texte si l'arbre n'y correspond plus : tout de suite si c'est rapide, sinon
    /// dans un thread.
    pub fn update(&mut self, text: &Rope) {
        if self.fresh || self.job.is_some() {
            return;
        }
        let Some(waker) = self.waker.clone() else {
            let started = Instant::now();
            let tree = parse(&mut self.parser, text, self.tree.as_ref(), None, None);
            self.last_parse = started.elapsed();
            self.parsed(tree);
            return;
        };
        if self.tree.is_none() && text.len_bytes() > SYNC_INITIAL_MAX {
            self.start_job(text, waker);
            return;
        }
        let started = Instant::now();
        let deadline = started + SYNC_BUDGET;
        let tree = parse(
            &mut self.parser,
            text,
            self.tree.as_ref(),
            Some(deadline),
            None,
        );
        self.last_parse = started.elapsed();
        match tree {
            Some(tree) => self.parsed(Some(tree)),
            None => self.start_job(text, waker),
        }
    }

    fn parsed(&mut self, tree: Option<Tree>) {
        self.tree = tree;
        self.fresh = true;
        self.version += 1;
    }

    /// Reprend l'analyse dans un thread, sur une copie du texte.
    fn start_job(&mut self, text: &Rope, waker: Waker) {
        let (sender, result) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let copy = text.clone();
        let old = self.tree.clone();
        let language = self.config.language.clone();
        let stop = Arc::clone(&cancel);
        let spawned = std::thread::Builder::new()
            .name("cupride-syntaxe".to_owned())
            .spawn(move || {
                let mut parser = Parser::new();
                if parser.set_language(&language).is_err() {
                    return;
                }
                let started = Instant::now();
                let tree = parse(&mut parser, &copy, old.as_ref(), None, Some(&stop));
                tracing::debug!(durée = ?started.elapsed(), "analyse syntaxique en arrière-plan");
                if !stop.load(Ordering::Relaxed) && sender.send(tree).is_ok() {
                    waker.wake();
                }
            });
        match spawned {
            Ok(_) => {
                self.job = Some(Job {
                    result,
                    cancel,
                    edits: Vec::new(),
                });
            }
            Err(error) => {
                tracing::warn!(%error, "thread d'analyse impossible : analyse sur place");
                let tree = parse(&mut self.parser, text, self.tree.as_ref(), None, None);
                self.parsed(tree);
            }
        }
    }

    /// Récupère l'arbre d'une analyse en arrière-plan terminée, puis réanalyse ce qui a été
    /// modifié entre-temps. Renvoie `true` si l'arbre a changé.
    pub fn poll(&mut self, text: &Rope) -> bool {
        let Some(job) = &self.job else {
            return false;
        };
        let tree = match job.result.try_recv() {
            Ok(tree) => tree,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => None,
        };
        self.finish_job(tree, text);
        true
    }

    /// Attend la fin de l'analyse en arrière-plan (tests, mesures).
    pub fn wait(&mut self, text: &Rope) {
        while let Some(job) = &self.job {
            let tree = job.result.recv().ok().flatten();
            self.finish_job(tree, text);
        }
    }

    fn finish_job(&mut self, tree: Option<Tree>, text: &Rope) {
        let Some(job) = self.job.take() else {
            return;
        };
        match tree {
            Some(mut tree) => {
                for edit in &job.edits {
                    tree.edit(edit);
                }
                self.tree = Some(tree);
                self.fresh = job.edits.is_empty();
            }
            // Analyse ratée : l'ancien arbre reste, décalé.
            None => self.fresh = false,
        }
        self.version += 1;
        if !self.fresh {
            self.update(text);
        }
    }

    /// Arbre syntaxique, s'il a pu être construit. Il peut être en retard sur le texte pendant
    /// une analyse en arrière-plan ([`Syntax::is_fresh`]).
    pub fn tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }

    /// L'arbre correspond au texte actuel.
    pub fn is_fresh(&self) -> bool {
        self.fresh
    }

    /// Une analyse est en cours en arrière-plan.
    pub fn is_parsing(&self) -> bool {
        self.job.is_some()
    }

    /// Numéro qui change à chaque changement de l'arbre.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Durée de la dernière analyse sur le thread principal.
    pub fn last_parse(&self) -> Duration {
        self.last_parse
    }

    /// Plus petit nœud nommé qui contient la plage `range` (en octets).
    pub fn named_node_at(&self, range: std::ops::Range<usize>) -> Option<Node<'_>> {
        self.tree()?
            .root_node()
            .named_descendant_for_byte_range(range.start, range.end)
    }
}

/// Analyse `text`, en réutilisant `old` (déjà mis à jour des modifications) s'il est donné.
/// L'analyse est abandonnée (`None`) passé `deadline`, ou quand `cancel` passe à vrai.
pub(crate) fn parse(
    parser: &mut Parser,
    text: &Rope,
    old: Option<&Tree>,
    deadline: Option<Instant>,
    cancel: Option<&AtomicBool>,
) -> Option<Tree> {
    let source = Source::new(text);
    let mut progress = |_: &tree_sitter::ParseState| {
        let late = deadline.is_some_and(|d| Instant::now() >= d);
        let cancelled = cancel.is_some_and(|c| c.load(Ordering::Relaxed));
        if late || cancelled {
            std::ops::ControlFlow::Break(())
        } else {
            std::ops::ControlFlow::Continue(())
        }
    };
    let options = ParseOptions::new().progress_callback(&mut progress);
    let tree = parser.parse_with_options(&mut |byte, _| source.at(byte), old, Some(options));
    if tree.is_none() {
        parser.reset();
    }
    tree
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;
    use crate::config_for;

    fn syntax(language: &str, text: &str) -> (Syntax, Rope) {
        let rope = Rope::from_text(text);
        let config = config_for(language).expect("langage intégré");
        (Syntax::new(config, &rope, None), rope)
    }

    fn apply(syntax: &mut Syntax, rope: &mut Rope, edit: Edit) {
        edit.apply(rope).unwrap();
        syntax.edited(rope, &[edit]);
    }

    /// Arbre obtenu en analysant `rope` depuis zéro.
    fn reference(language: &str, rope: &Rope) -> String {
        syntax(language, &rope.to_text())
            .0
            .tree()
            .unwrap()
            .root_node()
            .to_sexp()
    }

    #[test]
    fn analyse_rust() {
        let (syntax, _) = syntax("Rust", "fn main() {\n    println!(\"salut\");\n}\n");
        let root = syntax.tree().expect("arbre").root_node();
        assert_eq!(root.kind(), "source_file");
        assert!(!root.has_error());
        assert_eq!(root.named_child(0).map(|n| n.kind()), Some("function_item"));
        let node = syntax.named_node_at(3..4).expect("nœud");
        assert_eq!(node.kind(), "identifier");
        assert_eq!(node.parent().map(|n| n.kind()), Some("function_item"));
    }

    #[test]
    fn analyse_c_et_markdown() {
        let (c, _) = syntax("C", "int main(void) { return 0; }\n");
        let root = c.tree().expect("arbre").root_node();
        assert_eq!(root.kind(), "translation_unit");
        assert!(!root.has_error());
        let (md, _) = syntax("Markdown", "# Titre\n\nUn paragraphe.\n");
        assert_eq!(md.tree().expect("arbre").root_node().kind(), "document");
    }

    #[test]
    fn erreurs_signalees_dans_l_arbre() {
        let (syntax, _) = syntax("Rust", "fn main( {\n");
        assert!(syntax.tree().expect("arbre").root_node().has_error());
    }

    #[test]
    fn texte_sur_plusieurs_morceaux() {
        let text: String = (0..3000).map(|i| format!("fn f{i}() {{}}\n")).collect();
        let (syntax, _) = syntax("Rust", &text);
        let root = syntax.tree().expect("arbre").root_node();
        assert!(!root.has_error());
        assert_eq!(root.named_child_count(), 3000);
    }

    #[test]
    fn reanalyse_incrementale_identique_a_une_analyse_complete() {
        let (mut syntax, mut rope) = syntax("Rust", "fn a() {\n    let x = 1;\n}\n");
        let end = rope.len_bytes();
        apply(&mut syntax, &mut rope, Edit::insert(end, "struct B;\n"));
        let edit = Edit::replace(&rope, 17..22, "y = \"deux\"").unwrap();
        apply(&mut syntax, &mut rope, edit);
        apply(&mut syntax, &mut rope, Edit::insert(0, "// début\n"));
        assert!(syntax.is_fresh());
        let tree = syntax.tree().unwrap().root_node().to_sexp();
        assert_eq!(tree, reference("Rust", &rope));
        // Seule la zone modifiée a changé : la première fonction est réutilisée.
        let version = syntax.version();
        let end = rope.len_bytes();
        apply(&mut syntax, &mut rope, Edit::insert(end, "\n"));
        assert!(syntax.version() > version);
    }

    #[test]
    fn transaction_de_plusieurs_modifications() {
        let (mut syntax, mut rope) = syntax("C", "int a;\nint b;\nint c;\n");
        let edits = [
            Edit::insert(0, "long x;\n"),
            Edit::replace(&rope, 4..5, "zz").unwrap(),
        ];
        // La seconde modification est exprimée dans le texte après la première.
        let second = Edit::replace(&Rope::from_text("long x;\nint a;"), 12..13, "zz").unwrap();
        edits[0].apply(&mut rope).unwrap();
        second.apply(&mut rope).unwrap();
        syntax.edited(&mut rope, &[edits[0].clone(), second]);
        assert_eq!(rope.to_text(), "long x;\nint zz;\nint b;\nint c;\n");
        assert_eq!(
            syntax.tree().unwrap().root_node().to_sexp(),
            reference("C", &rope)
        );
    }

    #[test]
    fn analyse_en_arriere_plan() {
        // Un gros fichier : la première analyse se fait dans un thread.
        let text: String = (0..20_000)
            .map(|i| format!("int f{i}(int x) {{ return x * {i}; }}\n"))
            .collect();
        let mut rope = Rope::from_text(&text);
        let woken = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&woken);
        let waker = Waker::new(move || {
            counter.fetch_add(1, Ordering::Relaxed);
        });
        let config = config_for("C").unwrap();
        let mut syntax = Syntax::new(config, &rope, Some(waker));
        assert!(syntax.is_parsing());
        assert!(syntax.tree().is_none());
        // Une modification pendant l'analyse : appliquée à son arbre à l'arrivée.
        let edit = Edit::insert(0, "int premier;\n");
        edit.apply(&mut rope).unwrap();
        syntax.edited(&mut rope, &[edit]);
        syntax.wait(&rope);
        assert!(woken.load(Ordering::Relaxed) >= 1);
        assert!(syntax.is_fresh());
        let root = syntax.tree().unwrap().root_node();
        assert_eq!(root.named_child_count(), 20_001);
        assert_eq!(
            root.named_child(0).map(|n| n.kind()),
            Some("declaration"),
            "modification faite pendant l'analyse prise en compte"
        );
        // Ensuite, la frappe est réanalysée (sur place si c'est dans le budget, ce qui dépend
        // de la machine et du profil de compilation : voir le bench `parse`).
        let at = rope.line_to_byte(10_000).unwrap();
        let edit = Edit::insert(at, "int g;\n");
        edit.apply(&mut rope).unwrap();
        syntax.edited(&mut rope, &[edit]);
        syntax.wait(&rope);
        assert!(syntax.is_fresh());
        assert_eq!(
            syntax.tree().unwrap().root_node().named_child_count(),
            20_002
        );
    }
}
