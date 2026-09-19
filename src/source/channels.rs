//! Sources drawn by mixing channels into color, and how each channel is shown.
//!
//! A multichannel image paints each channel in a color of its own and adds
//! them up. Which are shown and how bright is what someone looking at it most
//! often wants to change, and nothing about that is particular to one format:
//! the sidebar's controls and the frame's number keys both write
//! [`SourceChannels`] on the source, and the format that owns it turns the
//! settings into pixels however it composites.

use bevy::prelude::*;

/// The brightest a channel can be turned up, as a multiple of how the dataset
/// publishes it. Four times is enough to lift a channel whose published window
/// was set for its brightest region into seeing its faint ones.
pub const MAX_GAIN: f32 = 4.0;

/// One channel of a source, and how it is shown.
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelSetting {
    pub label: String,
    /// The color it is painted in, as the dataset says. Data color, so it is
    /// not themed.
    pub color: [f32; 3],
    pub shown: bool,
    /// How bright, as a multiple of the dataset's own display window: 1 is as
    /// published, 2 reaches full brightness at half the intensity.
    pub gain: f32,
}

impl ChannelSetting {
    pub fn new(label: impl Into<String>, color: [f32; 3], shown: bool) -> Self {
        ChannelSetting {
            label: label.into(),
            color,
            shown,
            gain: 1.0,
        }
    }

    /// Whether it adds anything to the picture at all.
    pub fn contributes(&self) -> bool {
        self.shown && self.gain > 0.0
    }

    /// The window a channel published as `start..end` is displayed through at
    /// this gain: the start stays where it was, and the end comes down as the
    /// gain goes up.
    pub fn window(&self, start: f32, end: f32) -> (f32, f32) {
        // A channel at nothing is hidden rather than windowed, so the floor
        // only has to keep the division finite.
        let gain = self.gain.clamp(1e-3, MAX_GAIN);
        (start, start + (end - start) / gain)
    }
}

/// The channels a source mixes into color, and how the dataset published
/// them, which is what a reset goes back to.
#[derive(Component, Clone, Debug, PartialEq, Default)]
pub struct SourceChannels {
    pub channels: Vec<ChannelSetting>,
    published: Vec<ChannelSetting>,
}

impl SourceChannels {
    pub fn new(channels: Vec<ChannelSetting>) -> Self {
        SourceChannels {
            published: channels.clone(),
            channels,
        }
    }

    /// Show or hide channel `index`, returning whether it is shown now.
    pub fn toggle(&mut self, index: usize) -> Option<bool> {
        let channel = self.channels.get_mut(index)?;
        channel.shown = !channel.shown;
        Some(channel.shown)
    }

    /// Whether anything differs from how the dataset published it.
    pub fn changed_from_published(&self) -> bool {
        self.channels != self.published
    }

    pub fn reset(&mut self) {
        self.channels.clone_from(&self.published);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_published_the_window_is_left_alone() {
        let channel = ChannelSetting::new("red", [1.0, 0.0, 0.0], true);
        assert_eq!(channel.window(0.0, 1377.0), (0.0, 1377.0));
    }

    #[test]
    fn turning_a_channel_up_saturates_it_sooner() {
        let mut channel = ChannelSetting::new("green", [0.0, 1.0, 0.0], true);
        channel.gain = 2.0;
        assert_eq!(channel.window(100.0, 1100.0), (100.0, 600.0));
        // Past the ceiling it goes no further.
        channel.gain = 100.0;
        let (_, end) = channel.window(0.0, 4000.0);
        assert!((end - 4000.0 / MAX_GAIN).abs() < 1e-3);
    }

    #[test]
    fn a_channel_at_nothing_contributes_nothing() {
        let mut channel = ChannelSetting::new("blue", [0.0, 0.0, 1.0], true);
        channel.gain = 0.0;
        assert!(!channel.contributes());
        // And never divides by zero on the way there.
        assert!(channel.window(0.0, 10.0).1.is_finite());
    }

    #[test]
    fn toggling_flips_one_channel_and_says_which_way() {
        let mut channels = SourceChannels::new(vec![
            ChannelSetting::new("red", [1.0, 0.0, 0.0], true),
            ChannelSetting::new("green", [0.0, 1.0, 0.0], true),
        ]);
        assert_eq!(channels.toggle(1), Some(false));
        assert!(channels.channels[0].shown);
        assert_eq!(channels.toggle(5), None);
    }

    #[test]
    fn a_reset_puts_back_what_the_dataset_published() {
        let mut channels = SourceChannels::new(vec![
            ChannelSetting::new("red", [1.0, 0.0, 0.0], true),
            ChannelSetting::new("blue", [0.0, 0.0, 1.0], false),
        ]);
        assert!(!channels.changed_from_published());

        channels.channels[0].gain = 2.5;
        channels.toggle(1);
        assert!(channels.changed_from_published());

        channels.reset();
        assert!(!channels.changed_from_published());
        // Including a channel the dataset published hidden.
        assert!(!channels.channels[1].shown);
    }
}
