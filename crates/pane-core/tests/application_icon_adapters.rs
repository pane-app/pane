//! Each system's extraction of the applications' own icons (#172) against
//! the real system: on Windows the command interpreter's icon at 256 pixels
//! filling its box, a shortcut whose own icon holds only a small image
//! drawn by a fallback that fills its box, a shortcut's fingerprint
//! following its target's update or its own icon file's, and an inbox
//! packaged app's light and dark logos, which its fingerprint covers; on
//! macOS Calculator's bundle icon, its fingerprint covering its icon file;
//! and on every system a desktop entry's icon found in a fixture `hicolor`
//! theme placed on the data folders, as the Linux adapter looks it up.
//! These never touch the user's Start menu, Desktop, taskbar or registry,
//! and remove what they make.

use pane_core::applications::icons::theme::{IconThemes, entry_icon};

/// Decodes a PNG Pane encoded (8-bit RGBA, unfiltered rows, as
/// `pane_core::icons::encode_png` writes it): its width, height and
/// pixels.
#[cfg_attr(not(windows), allow(dead_code))]
fn decode_png(png: &[u8]) -> (u32, u32, Vec<u8>) {
    use std::io::Read;
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let (mut width, mut height, mut data) = (0, 0, Vec::new());
    let mut at = 8;
    while at + 8 <= png.len() {
        let length = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
        let kind = &png[at + 4..at + 8];
        let body = &png[at + 8..at + 8 + length];
        match kind {
            b"IHDR" => {
                width = u32::from_be_bytes(body[0..4].try_into().unwrap());
                height = u32::from_be_bytes(body[4..8].try_into().unwrap());
            }
            b"IDAT" => data.extend_from_slice(body),
            _ => {}
        }
        at += 12 + length;
    }
    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(&data[..])
        .read_to_end(&mut raw)
        .unwrap();
    let row = width as usize * 4;
    let rgba = raw
        .chunks_exact(row + 1)
        .flat_map(|line| line[1..].to_vec())
        .collect();
    (width, height, rgba)
}

/// The width and height a PNG states in its header.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn png_size(png: &[u8]) -> (u32, u32) {
    (
        u32::from_be_bytes(png[16..20].try_into().unwrap()),
        u32::from_be_bytes(png[20..24].try_into().unwrap()),
    )
}

#[test]
fn a_desktop_entrys_icon_is_found_in_the_themes_of_the_data_folders() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("share");
    let entry = data.join("applications/viewer.desktop");
    std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
    std::fs::write(
        &entry,
        "[Desktop Entry]\nType=Application\nName=Viewer\nName[vi]=Trình xem\n\
         Keywords=image;photo;\nExec=viewer %f\nIcon=org.example.Viewer\n",
    )
    .unwrap();
    let hicolor = data.join("icons/hicolor");
    for size in ["48x48", "256x256"] {
        let folder = hicolor.join(size).join("apps");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("org.example.Viewer.png"), b"png").unwrap();
    }
    std::fs::write(
        hicolor.join("index.theme"),
        "[Icon Theme]\nName=Hicolor\nDirectories=48x48/apps,256x256/apps\n\n\
         [48x48/apps]\nSize=48\nType=Threshold\n\n[256x256/apps]\nSize=256\nType=Threshold\n",
    )
    .unwrap();

    let name = entry_icon(&std::fs::read_to_string(&entry).unwrap()).unwrap();
    let themes = IconThemes::new(
        std::slice::from_ref(&data),
        Some(dir.path()),
        Some("Missing".into()),
    );

    assert_eq!(
        themes.find(&name),
        Some(hicolor.join("256x256/apps/org.example.Viewer.png")),
        "the size closest to 256, from hicolor after a theme that is not there"
    );
}

#[cfg(windows)]
mod windows {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use pane_core::applications::icons::{
        IconExtractor, NativeExtractor, covers_enough, file_fingerprint,
    };
    use pane_core::system_icons::SystemIcon;

    use super::decode_png;

    fn system32(name: &str) -> PathBuf {
        PathBuf::from(std::env::var("SystemRoot").unwrap())
            .join("System32")
            .join(name)
    }

    /// The pixels of an extracted icon.
    fn pixels(icon: &SystemIcon) -> (u32, u32, Vec<u8>) {
        match icon {
            SystemIcon::Png(png) => decode_png(png),
            SystemIcon::File(file) => panic!("a file, not pixels: {}", file.display()),
        }
    }

    #[test]
    fn the_command_interpreters_icon_extracts_at_256_pixels_filling_its_box() {
        let extracted = NativeExtractor
            .extract(&system32("cmd.exe").to_string_lossy())
            .unwrap();
        let (width, height, rgba) = pixels(&extracted.light);
        assert!(width >= 256 && height >= 256, "{width}×{height}");
        assert!(covers_enough(width, height, &rgba));
        assert!(extracted.dark.is_none());
    }

    /// An icon file holding only one 16-pixel image, 32 bits a pixel.
    fn small_icon(path: &Path) {
        let side = 16u32;
        let pixels = side * side * 4;
        let mask = side * 4; // 16 rows of 32 bits, all drawn
        let image = 40 + pixels + mask;
        let mut ico = vec![0, 0, 1, 0, 1, 0];
        ico.extend([side as u8, side as u8, 0, 0, 1, 0, 32, 0]);
        ico.extend(image.to_le_bytes());
        ico.extend(22u32.to_le_bytes());
        // BITMAPINFOHEADER, twice the height for the mask.
        ico.extend(40u32.to_le_bytes());
        ico.extend((side as i32).to_le_bytes());
        ico.extend((2 * side as i32).to_le_bytes());
        ico.extend(1u16.to_le_bytes());
        ico.extend(32u16.to_le_bytes());
        ico.extend([0u8; 24]);
        for _ in 0..side * side {
            ico.extend([0, 0, 255, 255]);
        }
        ico.extend(vec![0u8; mask as usize]);
        fs::write(path, ico).unwrap();
    }

    /// A shortcut to `target` whose own icon is `icon`, if it has one, made
    /// as the shell makes them.
    fn shortcut(path: &Path, target: &Path, icon: Option<&Path>) {
        let icon = match icon {
            Some(icon) => format!("$s.IconLocation = '{},0'; ", icon.display()),
            None => String::new(),
        };
        let script = format!(
            "$s = (New-Object -ComObject WScript.Shell).CreateShortcut('{}'); \
             $s.TargetPath = '{}'; {icon}$s.Save()",
            path.display(),
            target.display()
        );
        let status = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .status()
            .unwrap();
        assert!(status.success(), "the shortcut was not made");
    }

    #[test]
    fn a_shortcut_whose_icon_holds_only_a_small_image_is_drawn_by_a_fallback_filling_its_box() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("tool.exe");
        fs::copy(system32("cmd.exe"), &program).unwrap();
        let icon = dir.path().join("small.ico");
        small_icon(&icon);
        let link = dir.path().join("Tool.lnk");
        shortcut(&link, &program, Some(&icon));

        let extracted = NativeExtractor.extract(&link.to_string_lossy()).unwrap();

        let (width, height, rgba) = pixels(&extracted.light);
        assert!(
            covers_enough(width, height, &rgba),
            "a {width}×{height} icon padded around a small one"
        );
        // The fingerprint follows the shortcut.
        let before = NativeExtractor
            .fingerprint(&link.to_string_lossy())
            .unwrap();
        shortcut(&link, &system32("cmd.exe"), Some(&icon));
        let after = NativeExtractor
            .fingerprint(&link.to_string_lossy())
            .unwrap();
        assert_ne!(before, after);
    }

    /// Appends `bytes` zeros to the file at `path`, as an update rewriting
    /// it changes its size and modification time.
    fn grow(path: &Path, bytes: usize) {
        let mut contents = fs::read(path).unwrap();
        contents.resize(contents.len() + bytes, 0);
        fs::write(path, contents).unwrap();
    }

    /// The fingerprint of the shortcut at `link`, as Pane takes it.
    fn fingerprint(link: &Path) -> String {
        NativeExtractor
            .fingerprint(&link.to_string_lossy())
            .unwrap()
    }

    #[test]
    fn a_shortcuts_fingerprint_follows_its_target_or_its_own_icon_location() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("tool.exe");
        fs::copy(system32("cmd.exe"), &program).unwrap();
        let link = dir.path().join("Tool.lnk");
        shortcut(&link, &program, None);
        let before = fingerprint(&link);
        assert!(before.contains("tool.exe"), "{before}");

        // The program is updated in place; the shortcut is not touched.
        let shortcut_file = file_fingerprint(&link).unwrap();
        grow(&program, 16);
        assert_eq!(file_fingerprint(&link).unwrap(), shortcut_file);
        let updated = fingerprint(&link);
        assert_ne!(updated, before, "the target's update is seen");

        // With an icon location of its own, that file is followed, and the
        // target still is: extraction falls back to it when the location
        // yields no picture.
        let icon = dir.path().join("small.ico");
        small_icon(&icon);
        shortcut(&link, &program, Some(&icon));
        let with_icon = fingerprint(&link);
        assert!(with_icon.contains("small.ico"), "{with_icon}");
        assert!(with_icon.contains("tool.exe"), "{with_icon}");
        grow(&icon, 4);
        let icon_grown = fingerprint(&link);
        assert_ne!(icon_grown, with_icon, "the icon file changed");
        grow(&program, 16);
        assert_ne!(fingerprint(&link), icon_grown, "the target changed");
    }

    #[test]
    fn an_inbox_packaged_app_yields_light_and_dark_icons() {
        let candidates = [
            r"shell:AppsFolder\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App",
            r"shell:AppsFolder\windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel",
        ];
        let found: Vec<_> = candidates
            .iter()
            .filter_map(|source| Some((*source, NativeExtractor.extract(source).ok()?)))
            .collect();
        assert!(
            !found.is_empty(),
            "neither Calculator nor Settings has an icon"
        );
        // A package has a light theme's logo of its own only when it ships
        // a `lightunplated` variant; a server's inbox apps (CI's
        // windows-2025 has no Calculator, and its Settings ships none)
        // may have none, and then one logo serves both themes.
        for (source, extracted) in &found {
            if extracted.dark.is_none() {
                let SystemIcon::File(logo) = &extracted.light else {
                    panic!("{source}: the package's own logo is a file");
                };
                assert!(
                    !ships_a_light_logo(logo),
                    "{source} ships a light theme's logo, but has no dark variant"
                );
            }
        }
        let (source, extracted) = found
            .iter()
            .find(|(_, extracted)| extracted.dark.is_some())
            .unwrap_or(&found[0]);
        let source = *source;
        let dark = extracted.dark.clone().unwrap_or(extracted.light.clone());
        for icon in [&extracted.light, &dark] {
            match icon {
                SystemIcon::File(file) => {
                    assert!(file.is_file(), "{}", file.display());
                    assert!(
                        file.extension().is_some_and(|extension| extension == "png"),
                        "{}",
                        file.display()
                    );
                }
                SystemIcon::Png(_) => panic!("{source}: the package's own logo is a file"),
            }
        }
        if extracted.dark.is_some() {
            assert_ne!(extracted.light, dark, "{source}");
        }
        // Its fingerprint is its package's manifest and the logos drawn.
        let fingerprint = NativeExtractor.fingerprint(source).unwrap();
        assert!(fingerprint.contains("AppxManifest.xml"), "{fingerprint}");
        if let SystemIcon::File(logo) = &dark {
            let logo = logo.to_string_lossy();
            assert!(fingerprint.contains(&*logo), "{fingerprint}");
        }
    }

    /// Whether the folder of `logo`, a packaged app's logo file, holds a
    /// light theme's variant of it (`<logo>.…altform-lightunplated….png`).
    fn ships_a_light_logo(logo: &Path) -> bool {
        let name = logo.file_name().unwrap().to_string_lossy().to_lowercase();
        let stem = name.split('.').next().unwrap().to_owned();
        fs::read_dir(logo.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().to_lowercase())
            .any(|file| {
                file.starts_with(&format!("{stem}.")) && file.contains("altform-lightunplated")
            })
    }
}

#[cfg(target_os = "macos")]
#[test]
fn calculators_bundle_icon_is_extracted_at_256_pixels_or_more() {
    use std::path::PathBuf;

    use pane_core::applications::icons::{IconExtractor, NativeExtractor};
    use pane_core::system_icons::SystemIcon;

    let bundle = PathBuf::from("/System/Applications/Calculator.app");
    let extracted = NativeExtractor.extract(&bundle.to_string_lossy()).unwrap();
    let SystemIcon::Png(png) = &extracted.light else {
        panic!("not pixels: {:?}", extracted.light);
    };
    let (width, height) = png_size(png);
    assert!(width >= 256 && height >= 256, "{width}×{height}");
    // Its fingerprint is its Info.plist's and its icon file's.
    let fingerprint = NativeExtractor
        .fingerprint(&bundle.to_string_lossy())
        .unwrap();
    assert!(fingerprint.contains("Info.plist"), "{fingerprint}");
    assert!(fingerprint.contains("/Resources/"), "{fingerprint}");
}
