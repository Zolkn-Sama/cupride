//! Moteur de rendu : dessine une frame dans une image (fenêtre ou hors écran).

use glyphon::{Cache, Resolution, TextAtlas, TextRenderer, Viewport};

use crate::error::RenderError;
use crate::frame::Frame;
use crate::gpu::Gpu;
use crate::quad::QuadPipeline;
use crate::text::TextSystem;

/// Moteur de rendu pour un format d'image donné.
pub struct Renderer {
    format: wgpu::TextureFormat,
    atlas: TextAtlas,
    viewport: Viewport,
    /// Un moteur de texte par couche (créés au besoin) : chacun garde ses sommets préparés.
    text_renderers: Vec<TextRenderer>,
    quads: QuadPipeline,
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer")
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

impl Renderer {
    /// Moteur de rendu pour des images au format `format`.
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let cache = Cache::new(&gpu.device);
        let viewport = Viewport::new(&gpu.device, &cache);
        let mut atlas = TextAtlas::new(&gpu.device, &gpu.queue, &cache, format);
        let text_renderer = Self::text_renderer(gpu, &mut atlas);
        Self {
            format,
            atlas,
            viewport,
            text_renderers: vec![text_renderer],
            quads: QuadPipeline::new(&gpu.device, format),
        }
    }

    fn text_renderer(gpu: &Gpu, atlas: &mut TextAtlas) -> TextRenderer {
        TextRenderer::new(atlas, &gpu.device, wgpu::MultisampleState::default(), None)
    }

    /// Prépare le texte de chaque couche ; `Err(AtlasFull)` si l'atlas ne peut plus grandir.
    fn prepare_texts(
        &mut self,
        gpu: &Gpu,
        text: &mut TextSystem,
        frame: &Frame<'_>,
    ) -> Result<(), glyphon::PrepareError> {
        let (font_system, swash_cache) = text.parts();
        for (layer, renderer) in frame.layers.iter().zip(&mut self.text_renderers) {
            renderer.prepare(
                &gpu.device,
                &gpu.queue,
                font_system,
                &mut self.atlas,
                &self.viewport,
                layer.texts.iter().map(|item| item.to_area()),
                swash_cache,
            )?;
        }
        Ok(())
    }

    /// Format des images cibles.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Dessine `frame` dans `target`, une image de `width` × `height` pixels physiques.
    pub fn render(
        &mut self,
        gpu: &Gpu,
        text: &mut TextSystem,
        target: &wgpu::TextureView,
        (width, height): (u32, u32),
        frame: &Frame<'_>,
    ) -> Result<(), RenderError> {
        self.viewport
            .update(&gpu.queue, Resolution { width, height });
        while self.text_renderers.len() < frame.layers.len() {
            let renderer = Self::text_renderer(gpu, &mut self.atlas);
            self.text_renderers.push(renderer);
        }
        if let Err(glyphon::PrepareError::AtlasFull) = self.prepare_texts(gpu, text, frame) {
            // Atlas plein de glyphes encore utilisés : libérer ceux des frames précédentes.
            self.atlas.trim();
            self.prepare_texts(gpu, text, frame)
                .map_err(|e| RenderError::Draw(e.to_string()))?;
        }

        let groups: Vec<&[crate::quad::Quad]> = frame
            .layers
            .iter()
            .flat_map(|layer| [layer.quads.as_slice(), layer.overlays.as_slice()])
            .collect();
        let ranges = self
            .quads
            .prepare(&gpu.device, &gpu.queue, (width, height), &groups);

        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cupride-frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cupride-frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(frame.background.to_wgpu()),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // Couche par couche : fonds, texte, puis ce qui passe par-dessus le texte.
            for (index, renderer) in self
                .text_renderers
                .iter()
                .take(frame.layers.len())
                .enumerate()
            {
                self.quads.draw(&mut pass, ranges[2 * index].clone());
                renderer
                    .render(&self.atlas, &self.viewport, &mut pass)
                    .map_err(|e| RenderError::Draw(e.to_string()))?;
                self.quads.draw(&mut pass, ranges[2 * index + 1].clone());
            }
        }
        gpu.queue.submit(Some(encoder.finish()));
        // Les glyphes restent dans l'atlas ; ils ne sont seulement plus marqués « utilisés »,
        // et ne seront évincés que si la place manque.
        self.atlas.trim();
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::color::Color;
    use crate::geometry::Rect;
    use crate::offscreen::{OFFSCREEN_FORMAT, OffscreenTarget};
    use crate::quad::Quad;
    use crate::text::{Attrs, Buffer, Family, Metrics, Shaping, TextItem};

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

    /// Enregistre une capture dans `target/cupride-snapshots/`, pour la regarder.
    pub(crate) fn save_png(name: &str, (width, height): (u32, u32), pixels: &[u8]) {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/cupride-snapshots");
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join(format!("{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(pixels)
            .unwrap();
    }

    const BACKGROUND: Color = Color::rgb(0x1e, 0x1a, 0x1d);

    #[test]
    fn effacer_avec_la_couleur_de_fond() {
        let Some(gpu) = test_gpu() else { return };
        let target = OffscreenTarget::new(&gpu, 16, 8);
        let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
        let mut text = TextSystem::new();
        renderer
            .render(
                &gpu,
                &mut text,
                &target.view,
                target.size(),
                &Frame::new(BACKGROUND),
            )
            .unwrap();
        let pixels = target.read_rgba(&gpu).unwrap();
        for pixel in pixels.chunks(4) {
            for (got, want) in pixel.iter().zip([0x1e, 0x1a, 0x1d, 0xff]) {
                assert!(got.abs_diff(want) <= 1, "{pixel:?}");
            }
        }
    }

    #[test]
    fn texte_avec_ligatures_et_emojis() {
        let Some(gpu) = test_gpu() else { return };
        let (width, height) = (480, 72);
        let target = OffscreenTarget::new(&gpu, width, height);
        let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
        let mut text = TextSystem::new();
        let mut buffer = Buffer::new(text.font_system(), Metrics::new(28.0, 36.0));
        buffer.set_size(Some(width as f32), Some(height as f32));
        buffer.set_text(
            "fn main() -> é 😀 != fi",
            &Attrs::new().family(Family::Monospace),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(text.font_system(), false);
        let mut frame = Frame::new(BACKGROUND);
        frame.base().texts.push(TextItem {
            buffer: &buffer,
            left: 12.0,
            top: 18.0,
            clip: Rect::new(0.0, 0.0, width as f32, height as f32),
            color: Color::rgb(0xf2, 0xe6, 0xe3),
        });
        renderer
            .render(&gpu, &mut text, &target.view, target.size(), &frame)
            .unwrap();
        let pixels = target.read_rgba(&gpu).unwrap();
        save_png("texte", (width, height), &pixels);

        let differs = |p: &[u8]| p[0].abs_diff(0x1e) > 40 || p[1].abs_diff(0x1a) > 40;
        let drawn = pixels.chunks(4).filter(|p| differs(p)).count();
        assert!(drawn > 500, "presque rien n'a été dessiné : {drawn} pixels");
        // L'emoji est en couleur : des pixels nettement jaunes (rouge et vert forts, bleu faible).
        let yellow = pixels
            .chunks(4)
            .filter(|p| p[0] > 180 && p[1] > 140 && p[2] < 110)
            .count();
        if text.has_family("Apple Color Emoji") || text.has_family("Noto Color Emoji") {
            assert!(yellow > 20, "emoji non coloré : {yellow} pixels jaunes");
        }

        // Une seconde frame identique réutilise les glyphes déjà rasterisés dans l'atlas.
        renderer
            .render(&gpu, &mut text, &target.view, target.size(), &frame)
            .unwrap();
        assert_eq!(target.read_rgba(&gpu).unwrap(), pixels);
    }

    #[test]
    fn aucun_glyphe_rasterise_deux_fois() {
        let Some(gpu) = test_gpu() else { return };
        let (width, height) = (320, 48);
        let target = OffscreenTarget::new(&gpu, width, height);
        let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
        let mut text = TextSystem::new();
        let mut buffer = Buffer::new(text.font_system(), Metrics::new(20.0, 28.0));
        buffer.set_size(Some(width as f32), Some(height as f32));
        buffer.set_text(
            "let x = 42; // atlas",
            &Attrs::new().family(Family::Monospace),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(text.font_system(), false);
        let mut frame = Frame::new(BACKGROUND);
        frame.base().texts.push(TextItem {
            buffer: &buffer,
            left: 8.0,
            top: 10.0,
            clip: Rect::new(0.0, 0.0, width as f32, height as f32),
            color: Color::rgb(0xf2, 0xe6, 0xe3),
        });
        renderer
            .render(&gpu, &mut text, &target.view, target.size(), &frame)
            .unwrap();
        let first = target.read_rgba(&gpu).unwrap();

        // Sans aucune police, rien ne peut être rasterisé : une image identique prouve que
        // tous les glyphes viennent de l'atlas.
        let mut no_fonts = TextSystem::without_fonts();
        renderer
            .render(&gpu, &mut no_fonts, &target.view, target.size(), &frame)
            .unwrap();
        assert_eq!(target.read_rgba(&gpu).unwrap(), first);

        // Témoin : avec un atlas vide, le même système sans polices ne dessine rien.
        let mut fresh = Renderer::new(&gpu, OFFSCREEN_FORMAT);
        fresh
            .render(&gpu, &mut no_fonts, &target.view, target.size(), &frame)
            .unwrap();
        assert_ne!(target.read_rgba(&gpu).unwrap(), first);
    }

    #[test]
    fn rectangles_sous_et_par_dessus_le_texte() {
        let Some(gpu) = test_gpu() else { return };
        let (width, height) = (40, 20);
        let target = OffscreenTarget::new(&gpu, width, height);
        let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
        let mut text = TextSystem::new();
        let mut frame = Frame::new(BACKGROUND);
        frame.base().quads.push(Quad::new(
            Rect::new(0.0, 0.0, 20.0, 20.0),
            Color::rgb(0xff, 0x00, 0x00),
        ));
        // Semi-transparent, par-dessus le rouge : mélange.
        frame.base().overlays.push(Quad::new(
            Rect::new(10.0, 0.0, 20.0, 20.0),
            Color::rgba(0x00, 0x00, 0xff, 0x80),
        ));
        renderer
            .render(&gpu, &mut text, &target.view, target.size(), &frame)
            .unwrap();
        let pixels = target.read_rgba(&gpu).unwrap();
        let at = |x: usize, y: usize| {
            let i = (y * width as usize + x) * 4;
            [pixels[i], pixels[i + 1], pixels[i + 2]]
        };
        assert_eq!(at(5, 10), [0xff, 0x00, 0x00]);
        let mixed = at(15, 10);
        assert!(mixed[0] > 0x80 && mixed[2] > 0x80, "mélange : {mixed:?}");
        let over_bg = at(25, 10);
        assert!(
            over_bg[2] > 0x80 && over_bg[0] < 0x40,
            "bleu sur fond : {over_bg:?}"
        );
        let bg = at(35, 10);
        assert!(bg[0].abs_diff(0x1e) <= 1, "fond intact : {bg:?}");
    }

    #[test]
    fn une_couche_cache_le_texte_de_celle_du_dessous() {
        let Some(gpu) = test_gpu() else { return };
        let (width, height) = (200, 40);
        let target = OffscreenTarget::new(&gpu, width, height);
        let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
        let mut text = TextSystem::new();
        let mut buffer = Buffer::new(text.font_system(), Metrics::new(28.0, 36.0));
        buffer.set_size(Some(width as f32), Some(height as f32));
        buffer.set_text("MMMMMMMMMM", &Attrs::new(), Shaping::Advanced, None);
        buffer.shape_until_scroll(text.font_system(), false);
        let white = Color::rgb(0xff, 0xff, 0xff);
        let mut frame = Frame::new(BACKGROUND);
        frame.base().texts.push(TextItem {
            buffer: &buffer,
            left: 0.0,
            top: 0.0,
            clip: Rect::new(0.0, 0.0, width as f32, height as f32),
            color: white,
        });
        // Popup opaque sur la moitié droite.
        let popup = Color::rgb(0x00, 0x40, 0x00);
        frame
            .push_layer()
            .quads
            .push(Quad::new(Rect::new(100.0, 0.0, 100.0, 40.0), popup));
        renderer
            .render(&gpu, &mut text, &target.view, target.size(), &frame)
            .unwrap();
        let pixels = target.read_rgba(&gpu).unwrap();
        let white_in = |x0: usize, x1: usize| {
            (0..height as usize)
                .flat_map(|y| (x0..x1).map(move |x| (y * width as usize + x) * 4))
                .filter(|&i| pixels[i] > 0xc0 && pixels[i + 1] > 0xc0)
                .count()
        };
        assert!(white_in(0, 100) > 50, "texte visible à gauche");
        assert_eq!(white_in(100, 200), 0, "texte caché par la popup");
    }
}
