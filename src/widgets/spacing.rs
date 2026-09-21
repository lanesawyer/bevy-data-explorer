//! How far apart things are, in one place.
//!
//! Two layers. [`step`] is the scale: few enough values, far enough apart,
//! that neighboring steps look different. [`space`] names what a space is
//! for, and each name points at a step. UI code uses [`space`], never a step
//! and never a literal, so choosing a spacing is choosing what the space is,
//! and changing how roomy the app is is an edit here rather than across it.
//!
//! The steps are multiples of four, but the smallest, which lines up with
//! Feathers: its rows are 24px and its own gaps are four.
//!
//! A space that needs a new meaning gets a new name here rather than a step
//! at the call site. `a_spacing_is_named_rather_than_typed` fails on any
//! gap, padding or margin written as a number.

/// The scale every space is drawn from.
pub mod step {
    pub const XXS: f32 = 2.0;
    pub const XS: f32 = 4.0;
    pub const S: f32 = 8.0;
    pub const M: f32 = 12.0;
    pub const L: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
}

/// What a space is for.
pub mod space {
    use super::step;

    /// The join between controls drawn as one, such as a segmented button
    /// group. A seam rather than a space, so it sits off the scale.
    pub const SEAM: f32 = 1.0;

    /// Lines within one item: a name over its detail, a label over its value.
    pub const STACKED: f32 = step::XXS;
    /// An icon and the text beside it, inside one control.
    pub const ICON_LABEL: f32 = step::XS;
    /// Controls side by side in a row, like the buttons over a frame.
    pub const CONTROLS: f32 = step::XS;
    /// The items of a long list, where every pixel is multiplied by the
    /// number of rows.
    pub const LIST_ITEMS: f32 = step::XS;
    /// Rows of controls in a section or panel.
    pub const ROWS: f32 = step::S;
    /// What sets a heading apart from the rows around it, on top of the gap
    /// between them: a heading in a column spaced by [`ROWS`] then sits
    /// [`GROUPS`] below what comes before it.
    pub const HEADING: f32 = GROUPS - ROWS;
    /// Groups of rows within one panel.
    pub const GROUPS: f32 = step::M;
    /// How far a nested row sits in from the one it belongs to.
    pub const INDENT: f32 = step::M;

    /// The top and bottom of one item in a list, inside its own background.
    pub const ITEM_INSET: f32 = step::XS;
    /// Inside small boxes: wells, the status over a frame, a list item's
    /// sides, a section's body.
    pub const CONTROL_INSET: f32 = step::S;
    /// Inside a dock or a menu.
    pub const PANEL_INSET: f32 = step::M;
    /// Inside a screen laid over the window, such as help or settings.
    pub const SCREEN_INSET: f32 = step::L;

    /// Around and between the blocks of a whole-window screen, such as the
    /// welcome screen.
    pub const SCREEN_GAP: f32 = step::XL;
    /// Between columns that share a whole-window screen.
    pub const SCREEN_WIDE: f32 = step::XXL;
}

#[cfg(test)]
mod tests {
    use super::{space, step};

    const STEPS: [f32; 7] = [
        step::XXS,
        step::XS,
        step::S,
        step::M,
        step::L,
        step::XL,
        step::XXL,
    ];

    #[test]
    fn every_space_but_the_seam_is_a_step() {
        for named in [
            space::STACKED,
            space::ICON_LABEL,
            space::CONTROLS,
            space::LIST_ITEMS,
            space::ROWS,
            space::HEADING,
            space::GROUPS,
            space::INDENT,
            space::ITEM_INSET,
            space::CONTROL_INSET,
            space::PANEL_INSET,
            space::SCREEN_INSET,
            space::SCREEN_GAP,
            space::SCREEN_WIDE,
        ] {
            assert!(STEPS.contains(&named), "{named} is off the scale");
        }
    }

    #[test]
    fn neighboring_steps_are_far_enough_apart_to_tell_apart() {
        for pair in STEPS.windows(2) {
            assert!(pair[1] / pair[0] >= 4.0 / 3.0, "{pair:?} are too close");
        }
    }

    /// Whether `line` writes a gap, padding or margin as a number, inline or
    /// as a constant named for one.
    fn types_a_spacing(line: &str) -> bool {
        let code = line.split("//").next().unwrap_or("");
        if let Some((name, value)) = code
            .split_once("const ")
            .and_then(|(_, rest)| rest.split_once(": f32 = "))
        {
            let spacing = ["GAP", "INSET", "INDENT", "MARGIN"]
                .iter()
                .any(|word| name.contains(word))
                || (name.contains("PAD") && name.ends_with("_PX"));
            return spacing && value.trim_start().starts_with(|c: char| c.is_ascii_digit());
        }
        if !["gap", "padding", "margin"]
            .iter()
            .any(|word| code.contains(word))
        {
            return false;
        }
        code.match_indices("Val::Px(").any(|(at, found)| {
            let rest = code[at + found.len()..].trim_start_matches('-');
            rest.starts_with(|c: char| c.is_ascii_digit()) && !rest.starts_with("0.0)")
        })
    }

    #[test]
    fn the_check_finds_what_it_is_meant_to() {
        assert!(types_a_spacing("row_gap: { Val::Px(6.0) },"));
        assert!(types_a_spacing(
            "padding: UiRect::axes(Val::Px(space::ROWS), Val::Px(4.0)),"
        ));
        assert!(types_a_spacing("margin: { UiRect::top(Val::Px(-6.0)) },"));
        assert!(!types_a_spacing("row_gap: { Val::Px(space::ROWS) },"));
        assert!(!types_a_spacing("margin: UiRect::right(Val::Px(0.0)),"));
        assert!(!types_a_spacing("width: { Val::Px(320.0) },"));
        assert!(!types_a_spacing("// row_gap: Val::Px(6.0) was too loose"));
        assert!(types_a_spacing("const CHROME_GAP: f32 = 4.0;"));
        assert!(types_a_spacing("pub(super) const PAD_PX: f32 = 10.0;"));
        assert!(!types_a_spacing("const CHROME_GAP: f32 = space::CONTROLS;"));
        // Padding in the world's units rather than the screen's.
        assert!(!types_a_spacing("const CELL_PADDING: f32 = 0.06;"));
    }

    #[test]
    fn a_spacing_is_named_rather_than_typed() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut typed = Vec::new();
        let mut folders = vec![root];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    folders.push(path);
                    continue;
                }
                // This file writes numbers for the scale itself, and in the
                // examples the check is tested against.
                if path.extension().is_none_or(|ext| ext != "rs")
                    || path.ends_with("widgets/spacing.rs")
                {
                    continue;
                }
                let source = std::fs::read_to_string(&path).unwrap();
                for (number, line) in source.lines().enumerate() {
                    if types_a_spacing(line) {
                        typed.push(format!(
                            "{}:{}: {}",
                            path.display(),
                            number + 1,
                            line.trim()
                        ));
                    }
                }
            }
        }
        assert!(
            typed.is_empty(),
            "name these spacings with widgets::space instead:\n{}",
            typed.join("\n")
        );
    }
}
