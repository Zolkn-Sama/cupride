//! Rendu GPU de CuprIDE.
//!
//! - [`Gpu`] : carte graphique, device et file de commandes, partagés par toutes les fenêtres ;
//!   repli sur OpenGL si l'API native de l'OS n'est pas disponible.
//! - [`WindowSurface`] : l'image d'une fenêtre, redimensionnée avec elle.
//! - [`Renderer`] : dessine une frame.
//! - [`OffscreenTarget`] : rendu hors écran relu en mémoire, pour les tests et les captures.
//!
//! Cette crate ignore tout de l'éditeur : elle dessine des rectangles et du texte.

mod color;
mod error;
mod gpu;
mod offscreen;
mod renderer;
mod surface;

pub use color::Color;
pub use error::RenderError;
pub use gpu::Gpu;
pub use offscreen::{OFFSCREEN_FORMAT, OffscreenTarget};
pub use renderer::Renderer;
pub use surface::{Acquire, SurfaceFrame, WindowSurface};
pub use wgpu;
