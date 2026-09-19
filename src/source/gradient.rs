//! Continuous color scales, for coloring points by a numeric property.
//!
//! A [`Gradient`] is chosen per source (see `CellProperties::gradient`) rather
//! than baked into the streamers, so offering another is a matter of adding a
//! variant and its stops here.

use bevy::prelude::*;

/// A color scale running from a property's low values to its high ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Gradient {
    /// Perceptually uniform and readable with the common color deficiencies.
    #[default]
    Viridis,
}

/// Viridis at ten even steps, as `viridisLite::viridis(10)` gives it.
const VIRIDIS: [[u8; 3]; 10] = [
    [0x44, 0x01, 0x54],
    [0x48, 0x28, 0x78],
    [0x3e, 0x4a, 0x89],
    [0x31, 0x68, 0x8e],
    [0x26, 0x82, 0x8e],
    [0x1f, 0x9e, 0x89],
    [0x35, 0xb7, 0x79],
    [0x6d, 0xcd, 0x59],
    [0xb4, 0xde, 0x2c],
    [0xfd, 0xe7, 0x25],
];

impl Gradient {
    #[expect(dead_code, reason = "for the setting that will offer a choice")]
    pub const ALL: [Gradient; 1] = [Gradient::Viridis];

    #[expect(dead_code, reason = "for the setting that will offer a choice")]
    pub fn name(self) -> &'static str {
        match self {
            Gradient::Viridis => "Viridis",
        }
    }

    fn stops(self) -> &'static [[u8; 3]] {
        match self {
            Gradient::Viridis => &VIRIDIS,
        }
    }

    /// The color a fraction of the way along, clamped to the ends.
    ///
    /// Stops are blended in Oklab so the steps between them stay as even as
    /// the stops themselves.
    pub fn sample(self, fraction: f32) -> Color {
        let stops = self.stops();
        let at = |index: usize| {
            let [r, g, b] = stops[index];
            Oklaba::from(Srgba::rgb_u8(r, g, b))
        };
        let last = stops.len() - 1;
        let position = if fraction.is_nan() {
            0.0
        } else {
            fraction.clamp(0.0, 1.0) * last as f32
        };
        let below = (position.floor() as usize).min(last);
        let above = (below + 1).min(last);
        at(below).mix(&at(above), position - below as f32).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ends_are_the_first_and_last_stops() {
        let ends = |fraction| Srgba::from(Gradient::Viridis.sample(fraction)).to_hex();
        assert_eq!(ends(0.0), "#440154");
        assert_eq!(ends(1.0), "#FDE725");
        // Past either end clamps rather than extrapolating.
        assert_eq!(ends(-3.0), ends(0.0));
        assert_eq!(ends(7.0), ends(1.0));
    }

    #[test]
    fn viridis_grows_lighter_all_the_way_along() {
        // What makes it readable as a scale: order survives in lightness alone.
        let lightness = |fraction: f32| Oklaba::from(Gradient::Viridis.sample(fraction)).lightness;
        for step in 0..20 {
            let (a, b) = (step as f32 / 20.0, (step + 1) as f32 / 20.0);
            assert!(lightness(b) > lightness(a), "darker at {b} than at {a}");
        }
    }
}
