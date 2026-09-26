//! Cutting an image in a plane other than its own.
//!
//! A plane is named after the image's address, as `#plane=zy`; the reader
//! remaps its axes as it opens, so nothing past it knows what a plane is.

use ome_zarr_metadata::v0_4::AxisType;

use super::dataset::AxisLayout;

/// Which two of an image's spatial axes a frame shows, by name: the one
/// across the frame and the one down it. The third is paged through.
///
/// Every image opens looking down z, x across and y down. A volume imaged
/// whole can be cut any of three ways, and a viewer written for one — as
/// Neuroglancer lists its dimensions — may look along another; naming the
/// plane is what lets the same store be seen the way it was meant to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plane {
    pub across: char,
    pub down: char,
}

impl Plane {
    /// Two distinct axis letters, across then down, as `zy`.
    pub fn parse(text: &str) -> Option<Plane> {
        let mut letters = text.trim().chars().map(|c| c.to_ascii_lowercase());
        let (across, down) = (letters.next()?, letters.next()?);
        (letters.next().is_none()
            && across != down
            && across.is_alphabetic()
            && down.is_alphabetic())
        .then_some(Plane { across, down })
    }

    pub fn name(self) -> String {
        format!("{}{}", self.across, self.down)
    }

    /// The native plane, which needs no remapping.
    pub(super) fn is_native(self) -> bool {
        (self.across, self.down) == ('x', 'y')
    }
}

impl AxisLayout {
    /// This layout seen in `plane`: its across axis read as x, its down axis
    /// as y, and whichever spatial axis is left as z.
    pub(super) fn in_plane(
        self,
        axes: &[ome_zarr_metadata::v0_4::Axis],
        plane: Plane,
    ) -> Result<Self, String> {
        let named = |letter: char| {
            axes.iter()
                .position(|a| a.name.eq_ignore_ascii_case(&letter.to_string()))
                .ok_or_else(|| format!("the image has no {letter} axis to show"))
        };
        let (x, y) = (named(plane.across)?, named(plane.down)?);
        let spatial = |i: usize| {
            matches!(axes[i].r#type, Some(AxisType::Space))
                || ["x", "y", "z"].contains(&axes[i].name.to_ascii_lowercase().as_str())
        };
        if !spatial(x) || !spatial(y) {
            return Err(format!("{} is not a plane through space", plane.name()));
        }
        let z = (0..axes.len()).find(|&i| i != x && i != y && spatial(i));
        Ok(AxisLayout { x, y, z, ..self })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axes_of(fixture: &str) -> Vec<ome_zarr_metadata::v0_4::Axis> {
        let root: serde_json::Value = serde_json::from_str(fixture).unwrap();
        let attributes = root.get("attributes").unwrap_or(&root);
        crate::formats::image::store::parse_ome(attributes)
            .unwrap()
            .0[0]
            .axes
            .clone()
    }

    #[test]
    fn a_plane_is_two_axes_across_then_down() {
        assert_eq!(
            Plane::parse("ZY"),
            Some(Plane {
                across: 'z',
                down: 'y'
            })
        );
        assert_eq!(Plane::parse("zz"), None);
        assert_eq!(Plane::parse("xyz"), None);
        let (store, plane) = crate::formats::image::store::split_plane("https://h/a.zarr#plane=zy");
        assert_eq!(store, "https://h/a.zarr");
        assert_eq!(plane.map(Plane::name).as_deref(), Some("zy"));
        assert_eq!(
            crate::formats::image::store::split_plane("https://h/a.zarr").1,
            None
        );
    }

    #[test]
    fn a_plane_reads_its_axes_as_x_and_y_and_pages_the_third() {
        let axes = axes_of(include_str!("../../../testdata/root_zarr_v2_stack.json"));
        let native = AxisLayout::infer(&axes).unwrap();
        let (x, y, z) = (native.x, native.y, native.z.unwrap());
        let cut = native.in_plane(&axes, Plane::parse("zy").unwrap()).unwrap();
        assert_eq!((cut.x, cut.y, cut.z), (z, y, Some(x)));
        assert!(native.in_plane(&axes, Plane::parse("cy").unwrap()).is_err());
    }
}
