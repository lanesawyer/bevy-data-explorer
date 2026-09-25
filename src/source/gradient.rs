//! Continuous color scales, for coloring points by a numeric property.
//!
//! A [`Gradient`] is held per source in a [`ColorScale`] rather than baked
//! into the streamers, so offering another is a matter of adding a variant and
//! its stops here.
//!
//! The stops are matplotlib's, taken at even steps, except Viridis, which is
//! `viridisLite`'s and differs from it in the last bit here and there.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// A color scale running from a property's low values to its high ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gradient {
    /// Perceptually uniform and readable with the common color deficiencies.
    #[default]
    Viridis,
    Magma,
    Inferno,
    Plasma,
    /// Viridis reworked to read the same to red-green color blindness.
    Cividis,
    /// A rainbow without jet's false bands, for telling nearby values apart
    /// more than for reading order.
    Turbo,
    /// Diverging, through a neutral middle.
    Coolwarm,
    /// Diverging, red at the low end through white to blue.
    RdBu,
    Gray,
}

/// How a source's numeric coloring is drawn: which gradient, which way round,
/// and over what span.
///
/// A component of its own rather than a field of `CellProperties`, which a
/// catalog's service replaces whole when it answers.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorScale {
    pub gradient: Gradient,
    /// High values at the gradient's start rather than its end.
    pub reversed: bool,
    /// Span the gradient over the data's whole extent rather than the span a
    /// filter admits, so narrowing a range leaves the colors where they were.
    pub whole_extent: bool,
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

const MAGMA: [[u8; 3]; 10] = [
    [0x00, 0x00, 0x04],
    [0x18, 0x0f, 0x3d],
    [0x45, 0x10, 0x77],
    [0x72, 0x1f, 0x81],
    [0x9e, 0x2f, 0x7f],
    [0xcd, 0x40, 0x71],
    [0xf1, 0x60, 0x5d],
    [0xfd, 0x94, 0x67],
    [0xfe, 0xca, 0x8d],
    [0xfc, 0xfd, 0xbf],
];

const INFERNO: [[u8; 3]; 10] = [
    [0x00, 0x00, 0x04],
    [0x1b, 0x0c, 0x41],
    [0x4c, 0x0c, 0x6b],
    [0x78, 0x1c, 0x6d],
    [0xa5, 0x2c, 0x60],
    [0xcf, 0x44, 0x46],
    [0xed, 0x69, 0x25],
    [0xfb, 0x99, 0x06],
    [0xf7, 0xd1, 0x3d],
    [0xfc, 0xff, 0xa4],
];

const PLASMA: [[u8; 3]; 10] = [
    [0x0d, 0x08, 0x87],
    [0x46, 0x03, 0x9f],
    [0x74, 0x01, 0xa8],
    [0x9c, 0x17, 0x9e],
    [0xbd, 0x37, 0x86],
    [0xd8, 0x57, 0x6b],
    [0xed, 0x79, 0x53],
    [0xfa, 0x9e, 0x3b],
    [0xfd, 0xca, 0x26],
    [0xf0, 0xf9, 0x21],
];

const CIVIDIS: [[u8; 3]; 10] = [
    [0x00, 0x22, 0x4e],
    [0x12, 0x35, 0x70],
    [0x3c, 0x4a, 0x6c],
    [0x57, 0x5d, 0x6d],
    [0x70, 0x71, 0x73],
    [0x8a, 0x86, 0x78],
    [0xa5, 0x9c, 0x74],
    [0xc2, 0xb3, 0x69],
    [0xe1, 0xcc, 0x55],
    [0xfe, 0xe8, 0x38],
];

/// Sixteen steps rather than ten: its hue turns too sharply for fewer to
/// follow.
const TURBO: [[u8; 3]; 16] = [
    [0x30, 0x12, 0x3b],
    [0x41, 0x43, 0xa7],
    [0x47, 0x71, 0xe9],
    [0x3e, 0x9b, 0xfe],
    [0x22, 0xc5, 0xe2],
    [0x1a, 0xe4, 0xb6],
    [0x46, 0xf8, 0x84],
    [0x88, 0xff, 0x4e],
    [0xb9, 0xf6, 0x35],
    [0xe1, 0xdd, 0x37],
    [0xfa, 0xba, 0x39],
    [0xfd, 0x8d, 0x27],
    [0xf0, 0x5b, 0x12],
    [0xd6, 0x35, 0x06],
    [0xaf, 0x18, 0x01],
    [0x7a, 0x04, 0x03],
];

const COOLWARM: [[u8; 3]; 9] = [
    [0x3b, 0x4c, 0xc0],
    [0x62, 0x82, 0xea],
    [0x8d, 0xb0, 0xfe],
    [0xb8, 0xd0, 0xf9],
    [0xdd, 0xdd, 0xdd],
    [0xf5, 0xc4, 0xad],
    [0xf4, 0x9a, 0x7b],
    [0xde, 0x60, 0x4d],
    [0xb4, 0x04, 0x26],
];

/// ColorBrewer's eleven-class RdBu.
const RDBU: [[u8; 3]; 11] = [
    [0x67, 0x00, 0x1f],
    [0xb2, 0x18, 0x2b],
    [0xd6, 0x60, 0x4d],
    [0xf4, 0xa5, 0x82],
    [0xfd, 0xdb, 0xc7],
    [0xf7, 0xf7, 0xf7],
    [0xd1, 0xe5, 0xf0],
    [0x92, 0xc5, 0xde],
    [0x43, 0x93, 0xc3],
    [0x21, 0x66, 0xac],
    [0x05, 0x30, 0x61],
];

/// Blended in Oklab, so the two ends are all it needs to step evenly.
const GRAY: [[u8; 3]; 2] = [[0x00, 0x00, 0x00], [0xff, 0xff, 0xff]];

impl Gradient {
    pub const ALL: [Gradient; 9] = [
        Gradient::Viridis,
        Gradient::Magma,
        Gradient::Inferno,
        Gradient::Plasma,
        Gradient::Cividis,
        Gradient::Turbo,
        Gradient::Coolwarm,
        Gradient::RdBu,
        Gradient::Gray,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Gradient::Viridis => "Viridis",
            Gradient::Magma => "Magma",
            Gradient::Inferno => "Inferno",
            Gradient::Plasma => "Plasma",
            Gradient::Cividis => "Cividis",
            Gradient::Turbo => "Turbo",
            Gradient::Coolwarm => "Coolwarm",
            Gradient::RdBu => "RdBu",
            Gradient::Gray => "Gray",
        }
    }

    fn stops(self) -> &'static [[u8; 3]] {
        match self {
            Gradient::Viridis => &VIRIDIS,
            Gradient::Magma => &MAGMA,
            Gradient::Inferno => &INFERNO,
            Gradient::Plasma => &PLASMA,
            Gradient::Cividis => &CIVIDIS,
            Gradient::Turbo => &TURBO,
            Gradient::Coolwarm => &COOLWARM,
            Gradient::RdBu => &RDBU,
            Gradient::Gray => &GRAY,
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

    #[test]
    fn the_sequential_gradients_grow_lighter_all_the_way_along() {
        let sequential = [
            Gradient::Magma,
            Gradient::Inferno,
            Gradient::Plasma,
            Gradient::Cividis,
            Gradient::Gray,
        ];
        for gradient in sequential {
            let lightness = |fraction: f32| Oklaba::from(gradient.sample(fraction)).lightness;
            for step in 0..20 {
                let (a, b) = (step as f32 / 20.0, (step + 1) as f32 / 20.0);
                assert!(lightness(b) > lightness(a), "{gradient:?} darker at {b}");
            }
        }
    }

    #[test]
    fn a_scale_saved_by_an_older_version_keeps_what_it_says() {
        let scale: ColorScale = serde_json::from_str(r#"{"gradient":"rd_bu"}"#).unwrap();
        assert_eq!(scale.gradient, Gradient::RdBu);
        assert!(!scale.reversed && !scale.whole_extent);
    }
}
