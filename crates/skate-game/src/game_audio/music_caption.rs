//! The music-video caption (user ask 2026-11-10; docs/hails-additions/11-audio.md#music-banks): when a track from the
//! player's own music bank becomes audible, its title and artist show as a lower third along the bottom, fade in, hold
//! and fade out. Not for the disc soundtrack. The style is a resource a mod can change. Ours, not retail.
use bevy::prelude::*;

use super::music::MusicTrackStarted;

/// Timing and placement (seconds, percent of the window).
#[derive(Resource, Clone, Debug)]
pub(crate) struct MusicCaptionStyle {
    pub enabled: bool,
    pub fade_in: f32,
    pub hold: f32,
    pub fade_out: f32,
    pub left: f32,
    pub bottom: f32,
    pub title_size: f32,
    pub artist_size: f32,
    /// Mods' changes (`sdk.music.caption`), oldest first: the newest wins field by field; each ends with its mod.
    pub overrides: Vec<(String, skate_mods::MusicCaptionOptions)>,
}

impl Default for MusicCaptionStyle {
    fn default() -> Self {
        Self { enabled: true, fade_in: 0.5, hold: 6.0, fade_out: 0.8, left: 4.0, bottom: 9.0, title_size: 30.0, artist_size: 20.0,
            overrides: Vec::new() }
    }
}

impl MusicCaptionStyle {
    /// The style with the mods' changes applied.
    pub(crate) fn effective(&self) -> Self {
        let mut s = Self { overrides: Vec::new(), ..self.clone() };
        for (_, o) in &self.overrides {
            if let Some(v) = o.enabled { s.enabled = v; }
            if let Some(v) = o.fade_in { s.fade_in = v; }
            if let Some(v) = o.hold { s.hold = v; }
            if let Some(v) = o.fade_out { s.fade_out = v; }
            if let Some(v) = o.left { s.left = v; }
            if let Some(v) = o.bottom { s.bottom = v; }
            if let Some(v) = o.title_size { s.title_size = v; }
            if let Some(v) = o.artist_size { s.artist_size = v; }
        }
        s
    }

    /// Sets (Some) or removes (None) a mod's change.
    pub(crate) fn set(&mut self, owner: &str, options: Option<skate_mods::MusicCaptionOptions>) {
        self.overrides.retain(|(o, _)| o != owner);
        if let Some(o) = options {
            self.overrides.push((owner.to_owned(), o));
        }
    }
}

#[derive(Component)]
pub(crate) struct MusicCaption {
    age: f32,
}

#[derive(Component)]
pub(crate) struct CaptionText {
    full: f32,
}

#[derive(Component)]
pub(crate) struct CaptionBand;

/// The caption's opacity at `age` seconds (0 before, 1 while held, 0 after).
pub(crate) fn opacity(style: &MusicCaptionStyle, age: f32) -> f32 {
    if age < style.fade_in {
        (age / style.fade_in.max(1e-3)).clamp(0.0, 1.0)
    } else if age < style.fade_in + style.hold {
        1.0
    } else {
        (1.0 - (age - style.fade_in - style.hold) / style.fade_out.max(1e-3)).clamp(0.0, 1.0)
    }
}

/// A caption waiting for the script fonts (a title or artist beyond Latin).
#[derive(Resource, Default)]
pub(crate) struct PendingCaption(Option<MusicTrackStarted>);

#[allow(clippy::too_many_arguments)]
pub(super) fn show(mut commands: Commands, mut started: MessageReader<MusicTrackStarted>, style: Res<MusicCaptionStyle>,
                   time: Res<Time>, mut captions: Query<(Entity, &mut MusicCaption)>,
                   mut texts: Query<(&ChildOf, &CaptionText, &mut TextColor)>,
                   mut bands: Query<(Entity, &mut BackgroundColor), With<CaptionBand>>,
                   mut pending: ResMut<PendingCaption>, mut fonts: ResMut<crate::ui_fonts::FallbackFonts>) {
    let style = style.effective();
    if let Some(track) = started.read().filter(|t| t.custom && style.enabled).last() {
        pending.0 = Some(track.clone());
    }
    let waiting = pending.0.as_ref().is_some_and(|t| {
        crate::ui_fonts::needs_fallback(&t.title) || crate::ui_fonts::needs_fallback(&t.artist)
    }) && !fonts.ready();
    if waiting {
        fonts.request();
    }
    if let Some(track) = pending.0.take_if(|_| !waiting) {
        for (e, _) in &captions {
            commands.entity(e).despawn();
        }
        let band = commands.spawn((MusicCaption { age: 0.0 }, CaptionBand, GlobalZIndex(6),
            Node { position_type: PositionType::Absolute, left: percent(style.left), bottom: percent(style.bottom),
                flex_direction: FlexDirection::Column, padding: UiRect::axes(px(16), px(10)), ..default() },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)))).id();
        commands.entity(band).with_child((Text::new(track.title.clone()), TextFont { font: fonts.ui_font(), font_size: style.title_size, ..default() },
            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.0)), CaptionText { full: 1.0 }));
        if !track.artist.is_empty() {
            commands.entity(band).with_child((Text::new(track.artist.clone()), TextFont { font: fonts.ui_font(), font_size: style.artist_size, ..default() },
                TextColor(Color::srgba(0.85, 0.85, 0.85, 0.0)), CaptionText { full: 0.9 }));
        }
        return;
    }
    for (e, mut caption) in &mut captions {
        caption.age += time.delta_secs();
        let a = opacity(&style, caption.age);
        if caption.age > style.fade_in + style.hold + style.fade_out {
            commands.entity(e).despawn();
            continue;
        }
        if let Ok((_, mut bg)) = bands.get_mut(e) {
            bg.0 = Color::srgba(0.0, 0.0, 0.0, 0.55 * a);
        }
        for (parent, text, mut color) in &mut texts {
            if parent.parent() == e {
                color.0.set_alpha(text.full * a);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mods_caption_change_applies_field_by_field_and_ends_with_it() {
        let mut s = MusicCaptionStyle::default();
        s.set("a", Some(skate_mods::MusicCaptionOptions { hold: Some(2.0), title_size: Some(40.0), ..Default::default() }));
        s.set("b", Some(skate_mods::MusicCaptionOptions { hold: Some(3.0), ..Default::default() }));
        let e = s.effective();
        assert_eq!((e.hold, e.title_size, e.bottom), (3.0, 40.0, 9.0), "newest wins, unset fields keep");
        s.set("b", None);
        assert_eq!(s.effective().hold, 2.0);
        s.set("a", None);
        assert_eq!(s.effective().hold, 6.0);
    }

    #[test]
    fn the_caption_fades_in_holds_and_fades_out() {
        let s = MusicCaptionStyle::default();
        assert_eq!(opacity(&s, 0.0), 0.0);
        assert!((opacity(&s, 0.25) - 0.5).abs() < 1e-6);
        assert_eq!(opacity(&s, 3.0), 1.0);
        assert!((opacity(&s, 6.9) - 0.5).abs() < 1e-6);
        assert_eq!(opacity(&s, 7.5), 0.0);
    }
}
