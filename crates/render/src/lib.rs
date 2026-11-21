//! Rendu GPU de CuprIDE.
//!
//! - [`Gpu`] : carte graphique, device et file de commandes, partagés par toutes les fenêtres ;
//!   repli sur OpenGL si l'API native de l'OS n'est pas disponible.
//! - [`WindowSurface`] : l'image d'une fenêtre, redimensionnée avec elle.
//! - [`Frame`] : liste de dessin d'une frame, construite par l'interface ; [`Renderer`] la
//!   dessine.
//! - [`text`] : polices système, mise en forme (`cosmic-text`) et atlas de glyphes (`glyphon`).
//! - [`OffscreenTarget`] : rendu hors écran relu en mémoire, pour les tests et les captures.
//!
//! - [`Quad`] : rectangle plein (sélection, curseur), sous ou par-dessus le texte.
//!
//! Cette crate ignore tout de l'éditeur : elle dessine des rectangles et du texte.

mod color;
mod error;
mod frame;
mod geometry;
mod gpu;
mod offscreen;
mod quad;
mod renderer;
mod surface;
pub mod text;

pub use color::Color;
pub use error::RenderError;
pub use frame::Frame;
pub use geometry::Rect;
pub use gpu::Gpu;
pub use offscreen::{OFFSCREEN_FORMAT, OffscreenTarget};
pub use quad::Quad;
pub use renderer::Renderer;
pub use surface::{Acquire, SurfaceFrame, WindowSurface};
pub use text::{TextItem, TextSystem};
pub use wgpu;
