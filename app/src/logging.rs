//! Journalisation structurée de CuprIDE.
//!
//! - Le niveau se règle avec la variable d'environnement `CUPRIDE_LOG`, qui accepte la syntaxe
//!   des filtres `tracing-subscriber` (`debug`, `cupride_core=trace,warn`…).
//! - Par défaut, seuls les avertissements et les erreurs sont journalisés : rien n'est écrit
//!   tant que tout va bien, et en particulier rien au repos.
//! - Les logs sont écrits dans le dossier de cache de l'OS (`<cache>/logs/cupride.<date>.log`),
//!   avec une rotation quotidienne, et recopiés sur la sortie d'erreur.

use std::io::IsTerminal;
use std::path::PathBuf;

use directories::ProjectDirs;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Variable d'environnement qui règle le niveau des logs.
pub(crate) const ENV_VAR: &str = "CUPRIDE_LOG";

/// Niveau utilisé quand `CUPRIDE_LOG` est absente ou invalide.
const DEFAULT_FILTER: &str = "warn";

/// Nombre de fichiers de log conservés (un par jour).
const MAX_LOG_FILES: usize = 7;

/// Garde l'écriture des logs active : les derniers logs sont vidés sur disque à sa destruction.
/// Elle doit vivre jusqu'à la fin de `main`.
#[derive(Debug)]
#[must_use = "les logs ne sont plus écrits dans le fichier dès que la garde est détruite"]
pub(crate) struct LogGuard {
    _file: Option<WorkerGuard>,
}

/// Initialise la journalisation. À appeler une seule fois, au tout début de `main`.
pub(crate) fn init() -> LogGuard {
    let env_value = std::env::var(ENV_VAR).ok();
    let (filter, filter_error) = build_filter(env_value.as_deref());

    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal());

    let (file_layer, file_guard, file_error) = match open_log_file() {
        Ok((writer, path)) => {
            let (writer, guard) = tracing_appender::non_blocking(writer);
            let layer = tracing_subscriber::fmt::layer()
                .with_writer(writer)
                .with_ansi(false);
            tracing::trace!(dir = %path.display(), "dossier des logs");
            (Some(layer), Some(guard), None)
        }
        Err(error) => (None, None, Some(error)),
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();

    // Signalés une fois la journalisation en place, pour qu'ils soient visibles.
    if let Some(error) = filter_error {
        tracing::warn!(
            "valeur de {ENV_VAR} invalide ({error}), niveau par défaut « {DEFAULT_FILTER} » utilisé"
        );
    }
    if let Some(error) = file_error {
        tracing::warn!("logs écrits uniquement sur la sortie d'erreur : {error}");
    }

    LogGuard { _file: file_guard }
}

/// Construit le filtre à partir de la valeur de `CUPRIDE_LOG`. Une valeur absente, vide ou
/// invalide donne le filtre par défaut ; une valeur invalide renvoie aussi l'erreur à signaler.
fn build_filter(value: Option<&str>) -> (EnvFilter, Option<String>) {
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        None => (EnvFilter::new(DEFAULT_FILTER), None),
        Some(value) => match EnvFilter::try_new(value) {
            Ok(filter) => (filter, None),
            Err(error) => (EnvFilter::new(DEFAULT_FILTER), Some(error.to_string())),
        },
    }
}

/// Ouvre le fichier de log dans le dossier de cache de l'OS.
fn open_log_file() -> Result<(RollingFileAppender, PathBuf), String> {
    let dirs = ProjectDirs::from("", "", "cupride")
        .ok_or_else(|| "dossier personnel introuvable".to_owned())?;
    let dir = dirs.cache_dir().join("logs");
    // Créé d'abord : sinon l'appender signale le dossier absent en voulant purger les anciens logs.
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("impossible de créer {} : {error}", dir.display()))?;
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("cupride")
        .filename_suffix("log")
        .max_log_files(MAX_LOG_FILES)
        .build(&dir)
        .map_err(|error| format!("impossible d'ouvrir {} : {error}", dir.display()))?;
    Ok((appender, dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn niveau_par_defaut_sans_variable() {
        let (filter, error) = build_filter(None);
        assert_eq!(filter.to_string(), DEFAULT_FILTER);
        assert!(error.is_none());
    }

    #[test]
    fn variable_vide_donne_le_niveau_par_defaut() {
        let (filter, error) = build_filter(Some("  "));
        assert_eq!(filter.to_string(), DEFAULT_FILTER);
        assert!(error.is_none());
    }

    #[test]
    fn variable_valide_est_appliquee() {
        let (filter, error) = build_filter(Some("cupride_core=trace,info"));
        assert!(error.is_none());
        let text = filter.to_string();
        assert!(text.contains("cupride_core=trace"), "{text}");
        assert!(text.contains("info"), "{text}");
    }

    #[test]
    fn variable_invalide_donne_le_niveau_par_defaut_et_une_erreur() {
        let (filter, error) = build_filter(Some("cupride_core=pas-un-niveau"));
        assert_eq!(filter.to_string(), DEFAULT_FILTER);
        assert!(error.is_some());
    }
}
