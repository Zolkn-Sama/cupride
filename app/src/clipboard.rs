//! Presse-papiers de l'OS, via `arboard` : Windows, macOS, Linux (X11 et Wayland).

use cupride_core::clipboard::{Clipboard, ClipboardKind, MemoryClipboard};

/// La sélection primaire existe sur les Unix de bureau autres que macOS (X11, Wayland).
const HAS_PRIMARY: bool = cfg!(all(
    unix,
    not(any(
        target_os = "macos",
        target_os = "android",
        target_os = "emscripten"
    ))
));

/// Presse-papiers du système.
pub(crate) struct SystemClipboard {
    inner: arboard::Clipboard,
}

impl std::fmt::Debug for SystemClipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemClipboard").finish_non_exhaustive()
    }
}

impl SystemClipboard {
    /// Presse-papiers du système, ou presse-papiers interne au processus s'il est inaccessible
    /// (session sans affichage) : copier et coller restent possibles dans l'IDE.
    pub(crate) fn new_or_memory() -> Box<dyn Clipboard> {
        match arboard::Clipboard::new() {
            Ok(inner) => Box::new(Self { inner }),
            Err(error) => {
                tracing::warn!(
                    "presse-papiers du système inaccessible ({error}) : presse-papiers interne"
                );
                Box::new(MemoryClipboard::default())
            }
        }
    }
}

impl Clipboard for SystemClipboard {
    fn get(&mut self, kind: ClipboardKind) -> Option<String> {
        let result = match kind {
            ClipboardKind::Standard => self.inner.get_text(),
            ClipboardKind::Primary => primary::get(&mut self.inner)?,
        };
        match result {
            Ok(text) => Some(text),
            Err(arboard::Error::ContentNotAvailable) => None,
            Err(error) => {
                tracing::warn!("lecture du presse-papiers impossible : {error}");
                None
            }
        }
    }

    fn set(&mut self, kind: ClipboardKind, text: String) -> Result<(), String> {
        let result = match kind {
            ClipboardKind::Standard => self.inner.set_text(text),
            ClipboardKind::Primary => match primary::set(&mut self.inner, text) {
                Some(result) => result,
                None => return Ok(()),
            },
        };
        result.map_err(|error| error.to_string())
    }

    fn has_primary(&self) -> bool {
        HAS_PRIMARY
    }
}

/// Sélection primaire, sous Linux et les autres Unix de bureau.
#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
))]
mod primary {
    use arboard::{GetExtLinux, LinuxClipboardKind, SetExtLinux};

    pub(super) fn get(
        clipboard: &mut arboard::Clipboard,
    ) -> Option<Result<String, arboard::Error>> {
        Some(
            clipboard
                .get()
                .clipboard(LinuxClipboardKind::Primary)
                .text(),
        )
    }

    pub(super) fn set(
        clipboard: &mut arboard::Clipboard,
        text: String,
    ) -> Option<Result<(), arboard::Error>> {
        Some(
            clipboard
                .set()
                .clipboard(LinuxClipboardKind::Primary)
                .text(text),
        )
    }
}

/// Pas de sélection primaire sur cet OS : lecture vide, écriture ignorée.
#[cfg(not(all(
    unix,
    not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
)))]
mod primary {
    pub(super) fn get(_: &mut arboard::Clipboard) -> Option<Result<String, arboard::Error>> {
        None
    }

    pub(super) fn set(_: &mut arboard::Clipboard, _: String) -> Option<Result<(), arboard::Error>> {
        None
    }
}
