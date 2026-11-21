//! Notifications affichées en bas à droite, au-dessus de la barre d'état, puis retirées après
//! leur durée d'affichage (configurable par gravité) ou d'un clic.

use std::time::Instant;

use cupride_core::config::NotificationsConfig;
use cupride_core::notifications::{Level, Notifications};
use cupride_render::{Layer, Quad, Rect, TextSystem};

use crate::label::Label;
use crate::style::Style;
use crate::theme::Theme;

/// Nombre maximal de notifications affichées en même temps (les plus récentes).
pub const MAX_TOASTS: usize = 4;

#[derive(Debug)]
struct Toast {
    id: u64,
    level: Level,
    text: String,
    expires: Option<Instant>,
    label: Label,
    area: Rect,
}

/// Notifications affichées.
#[derive(Debug, Default)]
pub struct Toasts {
    items: Vec<Toast>,
    /// Identifiant de la dernière notification prise en compte.
    last_seen: u64,
}

impl Toasts {
    /// Prend les notifications émises depuis le dernier appel, avec leur durée d'affichage.
    /// Renvoie vrai s'il y en a de nouvelles.
    pub fn sync(
        &mut self,
        notifications: &Notifications,
        config: &NotificationsConfig,
        now: Instant,
    ) -> bool {
        let before = self.last_seen;
        for notification in notifications.since(self.last_seen) {
            // Une seule ligne affichée : le reste est dans l'historique.
            let mut text = notification
                .message
                .lines()
                .next()
                .unwrap_or_default()
                .to_owned();
            if notification.message.lines().nth(1).is_some() {
                text.push_str(" …");
            }
            self.items.push(Toast {
                id: notification.id,
                level: notification.level,
                text,
                expires: config.timeout(notification.level).map(|d| now + d),
                label: Label::default(),
                area: Rect::default(),
            });
            self.last_seen = notification.id;
        }
        let excess = self.items.len().saturating_sub(MAX_TOASTS);
        self.items.drain(..excess);
        self.last_seen != before
    }

    /// Retire les notifications expirées ; renvoie vrai s'il y en avait.
    pub fn expire(&mut self, now: Instant) -> bool {
        let before = self.items.len();
        self.items.retain(|t| t.expires.is_none_or(|e| e > now));
        self.items.len() != before
    }

    /// Prochaine expiration, pour programmer un réveil.
    pub fn next_expiry(&self) -> Option<Instant> {
        self.items.iter().filter_map(|t| t.expires).min()
    }

    /// Aucune notification affichée ?
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Identifiants affichés, du plus ancien au plus récent.
    pub fn shown(&self) -> Vec<u64> {
        self.items.iter().map(|t| t.id).collect()
    }

    /// Clic en (`x`, `y`) : retire la notification touchée ; renvoie vrai s'il y en avait une.
    pub fn dismiss_at(&mut self, x: f32, y: f32) -> bool {
        let before = self.items.len();
        self.items.retain(|t| !t.area.contains(x, y));
        self.items.len() != before
    }

    /// Met en forme les notifications et les empile, la plus récente en bas, dans le coin bas
    /// droit de `area`.
    pub fn prepare(&mut self, text: &mut TextSystem, style: &Style, area: Rect) {
        let padding = style.px(10.0);
        let stripe = style.px(4.0);
        let gap = style.px(6.0);
        let max_width = (area.width * 0.6).max(style.px(200.0)).min(area.width);
        let height = style.line_height + 2.0 * style.px(6.0);
        let mut bottom = area.bottom() - gap;
        for toast in self.items.iter_mut().rev() {
            toast
                .label
                .set(text, &toast.text, &style.family, style.metrics());
            let width = (toast.label.width() + 2.0 * padding + stripe).min(max_width);
            toast.area = Rect::new(
                (area.right() - gap - width).round(),
                (bottom - height).round(),
                width.round(),
                height,
            );
            bottom -= height + gap;
        }
    }

    /// Dessine les notifications dans `layer`.
    pub fn draw<'a>(&'a self, layer: &mut Layer<'a>, style: &Style, theme: &Theme) {
        let border = style.px(1.0).max(1.0);
        let stripe = style.px(4.0);
        for toast in &self.items {
            let a = toast.area;
            let color = match toast.level {
                Level::Info => theme.info,
                Level::Warning => theme.warning,
                Level::Error => theme.error,
            };
            layer.quads.push(Quad::new(a, theme.border));
            layer.quads.push(Quad::new(
                Rect::new(
                    a.x + border,
                    a.y + border,
                    a.width - 2.0 * border,
                    a.height - 2.0 * border,
                ),
                theme.popup,
            ));
            layer.quads.push(Quad::new(
                Rect::new(a.x + border, a.y + border, stripe, a.height - 2.0 * border),
                color,
            ));
            let left = a.x + stripe + style.px(10.0);
            layer.texts.extend(toast.label.item(
                left,
                a.y + style.px(6.0),
                Rect::new(left, a.y, a.right() - left - style.px(6.0), a.height),
                theme.foreground,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn affichage_et_expiration() {
        let t0 = Instant::now();
        let config = NotificationsConfig::default();
        let mut notifications = Notifications::default();
        let mut toasts = Toasts::default();
        assert!(!toasts.sync(&notifications, &config, t0));

        notifications.push(Level::Info, "enregistré");
        let error = notifications.push(Level::Error, "échec\ndétails");
        assert!(toasts.sync(&notifications, &config, t0));
        assert!(
            !toasts.sync(&notifications, &config, t0),
            "déjà prises en compte"
        );
        assert_eq!(toasts.items[1].text, "échec …", "première ligne seulement");
        assert_eq!(toasts.next_expiry(), Some(t0 + Duration::from_secs(4)));

        assert!(toasts.expire(t0 + Duration::from_secs(5)), "l'info expire");
        assert_eq!(toasts.shown(), [error], "l'erreur reste plus longtemps");
        assert!(toasts.expire(t0 + Duration::from_secs(9)));
        assert!(toasts.is_empty());

        // Sans expiration (0) : jusqu'à un clic.
        let sticky = NotificationsConfig {
            timeout_ms: 0,
            error_timeout_ms: 0,
        };
        for i in 0..6 {
            notifications.push(Level::Warning, format!("n{i}"));
        }
        toasts.sync(&notifications, &sticky, t0);
        assert_eq!(toasts.shown().len(), MAX_TOASTS, "les plus récentes");
        assert_eq!(toasts.next_expiry(), None);

        let mut text = TextSystem::new();
        let style = Style {
            family: "monospace".into(),
            font_size: 14.0,
            line_height: 20.0,
            scale: 1.0,
        };
        toasts.prepare(&mut text, &style, Rect::new(0.0, 0.0, 800.0, 600.0));
        let newest = toasts.items.last().unwrap().area;
        assert!(newest.bottom() <= 600.0 && newest.right() <= 800.0);
        assert!(toasts.items[0].area.y < newest.y, "la plus récente en bas");
        assert!(toasts.dismiss_at(newest.x + 5.0, newest.y + 5.0));
        assert_eq!(toasts.shown().len(), MAX_TOASTS - 1);
        assert!(!toasts.dismiss_at(1.0, 1.0));
    }
}
