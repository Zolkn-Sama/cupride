//! Surface d'une fenêtre : l'image dans laquelle on dessine, redimensionnée avec la fenêtre.

use wgpu::{DisplayAndWindowHandle, Surface};

use crate::error::RenderError;
use crate::gpu::Gpu;

type Recreate = Box<dyn Fn(&wgpu::Instance) -> Result<Surface<'static>, wgpu::CreateSurfaceError>>;

/// Surface d'une fenêtre et sa configuration.
pub struct WindowSurface {
    surface: Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    recreate: Recreate,
}

impl std::fmt::Debug for WindowSurface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowSurface")
            .field("format", &self.config.format)
            .field("width", &self.config.width)
            .field("height", &self.config.height)
            .finish_non_exhaustive()
    }
}

/// Résultat d'une demande d'image.
#[derive(Debug)]
pub enum Acquire {
    /// Image prête à dessiner.
    Frame(SurfaceFrame),
    /// Réessayer tout de suite (surface reconfigurée ou recréée, délai dépassé).
    Retry,
    /// Fenêtre masquée ou surface inutilisable : ne rien dessiner avant le prochain événement.
    Hidden,
}

/// Image de la fenêtre à dessiner pour cette frame.
#[derive(Debug)]
pub struct SurfaceFrame {
    texture: wgpu::SurfaceTexture,
    /// Vue sur l'image, à utiliser comme cible de rendu.
    pub view: wgpu::TextureView,
}

impl WindowSurface {
    /// Configure `surface`, créée pour `window`, à la taille donnée en pixels physiques.
    pub fn new<W>(
        gpu: &Gpu,
        surface: Surface<'static>,
        window: W,
        width: u32,
        height: u32,
    ) -> Result<Self, RenderError>
    where
        W: DisplayAndWindowHandle + Clone + 'static,
    {
        let capabilities = surface.get_capabilities(&gpu.adapter);
        // Format sRGB de préférence : les couleurs du thème sont données en sRGB.
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| capabilities.formats.first().copied())
            .ok_or(RenderError::NoSurfaceFormat)?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            // Synchronisé sur l'écran : aucune image inutile, pas de déchirure.
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
            // Une seule image d'avance : latence de frappe minimale.
            desired_maximum_frame_latency: 1,
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&gpu.device, &config);
        Ok(Self {
            surface,
            config,
            recreate: Box::new(move |instance| instance.create_surface(window.clone())),
        })
    }

    /// Format des images de la surface.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    /// Taille en pixels physiques.
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Adapte la surface à la nouvelle taille de la fenêtre. Une taille nulle (fenêtre réduite)
    /// est ignorée.
    pub fn resize(&mut self, gpu: &Gpu, width: u32, height: u32) {
        if width == 0 || height == 0 || (width, height) == self.size() {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&gpu.device, &self.config);
    }

    /// Image à dessiner pour la frame suivante.
    pub fn acquire(&mut self, gpu: &Gpu) -> Acquire {
        let texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => texture,
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                // Image utilisable, mais la surface doit être reconfigurée pour la suivante.
                self.surface.configure(&gpu.device, &self.config);
                texture
            }
            wgpu::CurrentSurfaceTexture::Occluded => return Acquire::Hidden,
            wgpu::CurrentSurfaceTexture::Timeout => return Acquire::Retry,
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&gpu.device, &self.config);
                return Acquire::Retry;
            }
            wgpu::CurrentSurfaceTexture::Lost => match (self.recreate)(&gpu.instance) {
                Ok(surface) => {
                    self.surface = surface;
                    self.surface.configure(&gpu.device, &self.config);
                    return Acquire::Retry;
                }
                Err(error) => {
                    tracing::error!("surface perdue, recréation impossible : {error}");
                    return Acquire::Hidden;
                }
            },
            wgpu::CurrentSurfaceTexture::Validation => {
                tracing::error!("erreur de validation en acquérant l'image de la fenêtre");
                return Acquire::Hidden;
            }
        };
        let view = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        Acquire::Frame(SurfaceFrame { texture, view })
    }

    /// Affiche l'image dessinée.
    pub fn present(&self, gpu: &Gpu, frame: SurfaceFrame) {
        gpu.queue.present(frame.texture);
    }
}
