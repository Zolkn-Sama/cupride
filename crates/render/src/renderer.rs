//! Moteur de rendu : dessine une frame dans une image (fenêtre ou hors écran).

use crate::color::Color;
use crate::gpu::Gpu;

/// Moteur de rendu pour un format d'image donné.
#[derive(Debug)]
pub struct Renderer {
    format: wgpu::TextureFormat,
}

impl Renderer {
    /// Moteur de rendu pour des images au format `format`.
    pub fn new(_gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        Self { format }
    }

    /// Format des images cibles.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Efface l'image avec la couleur de fond.
    pub fn clear(&mut self, gpu: &Gpu, target: &wgpu::TextureView, background: Color) {
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cupride-frame"),
            });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cupride-clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(background.to_wgpu()),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        gpu.queue.submit(Some(encoder.finish()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offscreen::{OFFSCREEN_FORMAT, OffscreenTarget};

    /// GPU de test, ou `None` si la machine n'en a pas (certaines machines de CI).
    pub(crate) fn test_gpu() -> Option<Gpu> {
        match Gpu::headless() {
            Ok(gpu) => Some(gpu),
            Err(error) => {
                eprintln!("test ignoré, pas de GPU : {error}");
                None
            }
        }
    }

    #[test]
    fn effacer_avec_la_couleur_de_fond() {
        let Some(gpu) = test_gpu() else { return };
        let target = OffscreenTarget::new(&gpu, 16, 8);
        let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
        renderer.clear(&gpu, &target.view, Color::rgb(0x2b, 0x1d, 0x24));
        let pixels = target.read_rgba(&gpu).unwrap();
        assert_eq!(pixels.len(), 16 * 8 * 4);
        for pixel in pixels.chunks(4) {
            // Aller-retour sRGB → linéaire → sRGB : à une unité près.
            for (got, want) in pixel.iter().zip([0x2b, 0x1d, 0x24, 0xff]) {
                assert!(got.abs_diff(want) <= 1, "{pixel:?}");
            }
        }
    }
}
