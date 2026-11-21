//! Ligne de commande interne : on y tape une commande (`file open src/main.rs`), Entrée
//! l'exécute.
//!
//! Elle s'affiche en bas de la fenêtre, par-dessus l'éditeur. Au-dessus de la ligne de saisie, le
//! résultat de la dernière commande : une erreur (la ligne reste ouverte, le texte est gardé pour
//! être corrigé) ou une sortie de plusieurs lignes (`file list`).

use cupride_render::{Layer, Quad, Rect, TextSystem};

use crate::label::Label;
use crate::style::Style;
use crate::text_input::TextInput;
use crate::theme::Theme;

/// Nombre maximal de lignes de résultat affichées (les dernières).
const MAX_MESSAGE_LINES: usize = 12;

/// Nature du message affiché au-dessus de la saisie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    /// Résultat d'une commande.
    Output,
    /// Erreur.
    Error,
}

/// Ligne de commande ouverte.
#[derive(Debug, Default)]
pub struct CommandLine {
    /// Texte saisi.
    pub input: TextInput,
    message: Option<(MessageKind, Vec<String>)>,
    /// Ligne d'aide sous la saisie (complétion, aide de la commande), si elle existe.
    hint: Option<String>,
    prompt: Label,
    text: Label,
    message_labels: Vec<Label>,
    hint_label: Label,
    area: Rect,
}

impl CommandLine {
    /// Ligne de commande préremplie avec `text`.
    pub fn new(text: &str) -> Self {
        Self {
            input: TextInput::with_text(text),
            ..Self::default()
        }
    }

    /// Affiche un message au-dessus de la saisie ; seules les dernières lignes sont gardées.
    pub fn set_message(&mut self, kind: MessageKind, text: &str) {
        let lines: Vec<String> = text.lines().map(ToOwned::to_owned).collect();
        let skip = lines.len().saturating_sub(MAX_MESSAGE_LINES);
        self.message = Some((kind, lines.into_iter().skip(skip).collect()));
    }

    /// Retire le message s'il s'agit d'une erreur : l'utilisateur corrige sa commande.
    pub fn clear_message_if_error(&mut self) {
        if matches!(self.message, Some((MessageKind::Error, _))) {
            self.message = None;
        }
    }

    /// Retire le message.
    pub fn clear_message(&mut self) {
        self.message = None;
    }

    /// Message affiché.
    pub fn message(&self) -> Option<(MessageKind, &[String])> {
        self.message.as_ref().map(|(k, l)| (*k, l.as_slice()))
    }

    /// Ligne d'aide affichée sous la saisie.
    pub fn set_hint(&mut self, hint: Option<String>) {
        self.hint = hint;
    }

    fn message_lines(&self) -> usize {
        self.message.as_ref().map_or(0, |(_, lines)| lines.len())
    }

    /// Hauteur occupée, en pixels physiques.
    pub fn height(&self, style: &Style) -> f32 {
        let lines = 1 + self.message_lines() + usize::from(self.hint.is_some());
        lines as f32 * style.line_height + 2.0 * style.px(6.0)
    }

    /// Met en forme les textes, et place la ligne en bas de `window` (au-dessus de `bottom`
    /// pixels réservés, la barre d'état).
    pub fn prepare(&mut self, text: &mut TextSystem, style: &Style, window: Rect, bottom: f32) {
        let height = self.height(style).min(window.height);
        self.area = Rect::new(
            window.x,
            (window.bottom() - bottom - height).max(window.y),
            window.width,
            height,
        );
        let metrics = style.metrics();
        self.prompt.set(text, ":", &style.family, metrics);
        self.text
            .set(text, self.input.text(), &style.family, metrics);
        let lines: Vec<String> = self
            .message
            .as_ref()
            .map(|(_, l)| l.clone())
            .unwrap_or_default();
        self.message_labels.resize_with(lines.len(), Label::default);
        for (label, line) in self.message_labels.iter_mut().zip(&lines) {
            label.set(text, line, &style.family, metrics);
        }
        if let Some(hint) = &self.hint {
            self.hint_label.set(text, hint, &style.family, metrics);
        }
    }

    /// Zone occupée, après [`CommandLine::prepare`].
    pub fn area(&self) -> Rect {
        self.area
    }

    fn input_left(&self, style: &Style) -> f32 {
        self.area.x + style.px(10.0) + self.prompt.width() + style.px(6.0)
    }

    fn input_top(&self, style: &Style) -> f32 {
        self.area.y + style.px(6.0) + self.message_lines() as f32 * style.line_height
    }

    /// Rectangle du curseur de saisie (pour placer la fenêtre de l'IME).
    pub fn cursor_rect(&self, style: &Style) -> Rect {
        let x = self.input_left(style) + self.text.x_at(self.input.cursor());
        Rect::new(
            x.round(),
            self.input_top(style),
            style.px(2.0).max(1.0),
            style.line_height,
        )
    }

    /// Dessine la ligne dans `layer`.
    pub fn draw<'a>(
        &'a self,
        layer: &mut Layer<'a>,
        style: &Style,
        theme: &Theme,
        cursor_visible: bool,
    ) {
        let area = self.area;
        layer.quads.push(Quad::new(area, theme.panel));
        layer.quads.push(Quad::new(
            Rect::new(area.x, area.y, area.width, style.px(1.0).max(1.0)),
            theme.border,
        ));
        let left = area.x + style.px(10.0);
        let mut top = area.y + style.px(6.0);
        if let Some((kind, _)) = &self.message {
            let color = match kind {
                MessageKind::Output => theme.foreground,
                MessageKind::Error => theme.error,
            };
            for label in &self.message_labels {
                layer.texts.extend(label.item(left, top, area, color));
                top += style.line_height;
            }
        }
        layer
            .texts
            .extend(self.prompt.item(left, top, area, theme.accent));
        let input_left = self.input_left(style);
        let selection = self.input.selection();
        if !selection.is_empty() {
            let x0 = self.text.x_at(selection.start);
            let x1 = self.text.x_at(selection.end);
            layer.quads.push(Quad::new(
                Rect::new(input_left + x0, top, x1 - x0, style.line_height).intersect(&area),
                theme.selection,
            ));
        }
        layer
            .texts
            .extend(self.text.item(input_left, top, area, theme.foreground));
        if cursor_visible {
            layer.overlays.push(Quad::new(
                self.cursor_rect(style).intersect(&area),
                theme.cursor,
            ));
        }
        if self.hint.is_some() {
            top += style.line_height;
            layer
                .texts
                .extend(self.hint_label.item(input_left, top, area, theme.muted));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> Style {
        Style {
            family: "monospace".into(),
            font_size: 14.0,
            line_height: 20.0,
            scale: 1.0,
        }
    }

    #[test]
    fn placement_et_messages() {
        let mut text = TextSystem::new();
        let theme = Theme::default();
        let window = Rect::new(0.0, 0.0, 800.0, 600.0);
        let mut line = CommandLine::new("file list");
        line.prepare(&mut text, &style(), window, 30.0);
        assert_eq!(line.area().bottom(), 570.0, "au-dessus de la barre d'état");
        assert_eq!(line.area().height, 20.0 + 12.0);

        let output: String = (1..=20).map(|i| format!("ligne {i}\n")).collect();
        line.set_message(MessageKind::Output, &output);
        assert_eq!(
            line.message().unwrap().1.len(),
            12,
            "les 12 dernières lignes"
        );
        assert_eq!(line.message().unwrap().1[0], "ligne 9");
        line.prepare(&mut text, &style(), window, 30.0);
        assert_eq!(line.area().height, 13.0 * 20.0 + 12.0);

        line.set_message(MessageKind::Error, "commande inconnue");
        line.prepare(&mut text, &style(), window, 30.0);
        let mut layer = cupride_render::Layer::default();
        line.draw(&mut layer, &style(), &theme, true);
        assert!(layer.texts.iter().any(|t| t.color == theme.error));
        assert_eq!(layer.overlays.len(), 1, "curseur");
        let cursor = line.cursor_rect(&style());
        assert!(
            cursor.x > line.area().x + 20.0,
            "après l'invite et le texte"
        );
    }
}
