//! The desktop's accent color, asked of the platform once at startup and
//! again whenever settings turns following it back on.
//!
//! Each platform keeps it somewhere different, and each is read by asking the
//! tool the platform ships rather than through a binding to its API:
//!
//! - Linux: the XDG desktop portal's `org.freedesktop.appearance` setting,
//!   which GNOME and KDE both answer, through `busctl` or else `gdbus`.
//! - macOS: `AppleAccentColor` in the global defaults, one of eight fixed
//!   colors. Unset means "multicolor", which leaves the choice to the app.
//! - Windows: the DWM `AccentColor` value in the registry.
//!
//! Asked off the main thread, since a session bus with no portal on it can
//! take seconds to say so. Until it answers, and whenever it cannot, the theme
//! keeps Feathers' own accent.
//!
//! Only read, never watched: changing the accent on the desktop reaches the
//! app at its next launch, or when the setting is turned off and on.

use std::process::Command;

use bevy::prelude::*;

/// The accent, if the platform has one set and could be asked.
pub fn system_accent() -> Option<Color> {
    if cfg!(target_os = "macos") {
        let out = run("defaults", &["read", "-g", "AppleAccentColor"])?;
        apple_accent(out.trim().parse().ok()?)
    } else if cfg!(windows) {
        let out = run(
            "reg",
            &[
                "query",
                r"HKCU\Software\Microsoft\Windows\DWM",
                "/v",
                "AccentColor",
            ],
        )?;
        windows_accent(&out)
    } else {
        const PORTAL: [&str; 3] = [
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings",
        ];
        run(
            "busctl",
            &[
                "--user",
                "--timeout=2",
                "call",
                PORTAL[0],
                PORTAL[1],
                PORTAL[2],
                "ReadOne",
                "ss",
                "org.freedesktop.appearance",
                "accent-color",
            ],
        )
        .or_else(|| {
            run(
                "gdbus",
                &[
                    "call",
                    "--session",
                    "--timeout=2",
                    "--dest",
                    PORTAL[0],
                    "--object-path",
                    PORTAL[1],
                    "--method",
                    &format!("{}.ReadOne", PORTAL[2]),
                    "org.freedesktop.appearance",
                    "accent-color",
                ],
            )
        })
        .and_then(|out| portal_accent(&out))
    }
}

/// What `program` printed, if it ran and succeeded.
fn run(program: &str, args: &[&str]) -> Option<String> {
    let mut command = Command::new(program);
    command.args(args);
    // Without this a windowed app flashes a console for the moment `reg` runs.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command.output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The portal's answer, as either tool prints it: `v (ddd) 0.56 0.25 0.67`
/// from `busctl`, `(<<(0.56, 0.25, 0.67)>>,)` from `gdbus`. The portal says a
/// component outside 0 to 1 means no accent is set.
fn portal_accent(out: &str) -> Option<Color> {
    let parts: Vec<f32> = out
        .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .filter_map(|word| word.parse().ok())
        .collect();
    let [r, g, b] = parts[..] else {
        return None;
    };
    [r, g, b]
        .iter()
        .all(|part| (0.0..=1.0).contains(part))
        .then(|| Color::srgb(r, g, b))
}

/// The registry's `0xAABBGGRR`, as `reg query` prints it.
fn windows_accent(out: &str) -> Option<Color> {
    let hex = out
        .split_whitespace()
        .find_map(|word| word.strip_prefix("0x"))?;
    let abgr = u32::from_str_radix(hex, 16).ok()?;
    let [r, g, b, _] = abgr.to_le_bytes();
    Some(Color::srgb_u8(r, g, b))
}

/// macOS's accent by its number, in the system's own colors. Graphite is -1;
/// "multicolor" writes nothing at all, and so never gets here.
fn apple_accent(index: i32) -> Option<Color> {
    let (r, g, b) = match index {
        -1 => (140, 140, 140),
        0 => (255, 59, 48),
        1 => (255, 149, 0),
        2 => (255, 204, 0),
        3 => (40, 205, 65),
        4 => (0, 122, 255),
        5 => (175, 82, 222),
        6 => (255, 45, 85),
        _ => return None,
    };
    Some(Color::srgb_u8(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_portal_is_read_from_either_tool() {
        let purple = Color::srgb(0.568627, 0.254902, 0.67451);
        assert_eq!(
            portal_accent("v (ddd) 0.568627 0.254902 0.67451\n"),
            Some(purple)
        );
        assert_eq!(
            portal_accent("(<<(0.568627, 0.254902, 0.67451)>>,)\n"),
            Some(purple)
        );
        assert_eq!(portal_accent("v (ddd) -1 -1 -1\n"), None, "unset");
        assert_eq!(portal_accent(""), None);
    }

    #[test]
    fn windows_keeps_it_backwards() {
        let out = "\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\DWM\n    \
                   AccentColor    REG_DWORD    0xffd77800\n\n";
        assert_eq!(windows_accent(out), Some(Color::srgb_u8(0x00, 0x78, 0xd7)));
        assert_eq!(windows_accent("nothing here"), None);
    }

    #[test]
    fn macos_names_its_accents_by_number() {
        assert_eq!(apple_accent(4), Some(Color::srgb_u8(0, 122, 255)));
        assert_eq!(apple_accent(9), None);
    }
}
