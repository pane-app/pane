//! A Windows packaged (AppX/MSIX) app's logo: the pure rules, compiled and
//! tested on every system. The Windows adapter finds the package's install
//! folder through the documented package functions and reads its manifest;
//! these rules say which file of the folder is the app's icon.
//!
//! The manifest (`AppxManifest.xml`) names each app's logo on its
//! `VisualElements`: `Square44x44Logo`, the one the Start menu's list and
//! the taskbar draw ([`manifest_logo`]). The package ships it in variants
//! whose qualifiers are in the file names
//! (`Square44x44Logo.targetsize-256_altform-unplated.png`): a target size in
//! pixels or a scale, and an alternate form. An unplated variant is drawn
//! without the tile colour behind it, for the dark theme; a
//! `lightunplated` one for the light theme ([`pick_logos`]).

/// The size, in pixels, the logo is chosen closest to: the size Pane
/// extracts every application's icon at.
const WANTED: u32 = crate::system_icons::ICON_SIZE;

/// The logo the manifest `manifest` names for its app with id `app`
/// (`App` in `Microsoft.WindowsCalculator_8wekyb3d8bbwe!App`), as a path
/// relative to the package's folder (`Assets\CalculatorAppList.png`): its
/// `Square44x44Logo`, else its `Square150x150Logo`, else the package's own
/// `Logo`. `None` when the manifest has no such app or names no logo.
pub fn manifest_logo(manifest: &str, app: &str) -> Option<String> {
    let mut rest = manifest;
    while let Some(at) = rest.find("<Application") {
        let element = &rest[at..];
        rest = &element["<Application".len()..];
        // `<Applications>` is the list, not an app.
        if !rest.starts_with(|c: char| c.is_whitespace()) {
            continue;
        }
        let head_end = rest.find('>').unwrap_or(rest.len());
        let matches =
            attribute(&rest[..head_end], "Id").is_some_and(|id| id.eq_ignore_ascii_case(app));
        if !matches {
            continue;
        }
        let body_end = rest.find("</Application>").unwrap_or(rest.len());
        let body = &rest[..body_end];
        return attribute(body, "Square44x44Logo")
            .or_else(|| attribute(body, "Square150x150Logo"))
            .or_else(|| element_text(manifest, "Logo"))
            .filter(|logo| !logo.trim().is_empty());
    }
    None
}

/// The value of the attribute `name` in `text`, the first one found, with
/// the five XML entities decoded.
fn attribute(text: &str, name: &str) -> Option<String> {
    let mut rest = text;
    loop {
        let at = rest.find(name)?;
        let before = rest[..at].chars().next_back();
        let after = &rest[at + name.len()..];
        rest = after;
        // A whole attribute name, not the end of another one.
        if before.is_some_and(|c| !c.is_whitespace()) {
            continue;
        }
        let after = after.trim_start();
        let Some(after) = after.strip_prefix('=') else {
            continue;
        };
        let after = after.trim_start();
        let quote = after.chars().next()?;
        if quote != '"' && quote != '\'' {
            continue;
        }
        let value = &after[1..];
        let end = value.find(quote)?;
        return Some(unescape(&value[..end]));
    }
}

/// The text of the first element `name` in `text` (`<Logo>…</Logo>`).
fn element_text(text: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&format!("</{name}>"))? + start;
    Some(unescape(text[start..end].trim()))
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// One file of a logo's variants, as its name qualifies it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Variant {
    name: String,
    /// Its size in pixels: its target size, or the logo's 44 (or 150)
    /// pixels at its scale.
    size: u32,
    /// Whether it is a target-size variant, drawn as it is at that size.
    target: bool,
    /// Its alternate form (`unplated`, `lightunplated`), if any.
    form: Option<String>,
}

/// The files to draw for the logo `logo` (its file name as the manifest
/// names it, `Square44x44Logo.png`) among `files`, the names in its
/// folder: the one for the dark theme, and the light theme's when the
/// package ships one of its own (`None` when one file serves both).
///
/// For each theme the target size closest to 256 pixels is chosen (the
/// smallest at least that large, else the largest), the unplated variant
/// for the dark theme and the `lightunplated` one for the light, else a
/// plated target size, else the largest scale, else the file as named.
/// High-contrast variants are left out. `None` when none is there.
pub fn pick_logos(logo: &str, files: &[String]) -> Option<(String, Option<String>)> {
    let (stem, extension) = logo.rsplit_once('.').unwrap_or((logo, ""));
    let base_size = if stem.contains("150x150") { 150 } else { 44 };
    let variants: Vec<Variant> = files
        .iter()
        .filter_map(|name| variant(name, stem, extension, base_size))
        .collect();
    let best = |keep: &dyn Fn(&Variant) -> bool| -> Option<String> {
        variants
            .iter()
            .filter(|variant| keep(variant))
            .min_by_key(|variant| {
                if variant.size >= WANTED {
                    (0, variant.size - WANTED)
                } else {
                    (1, WANTED - variant.size)
                }
            })
            .map(|variant| variant.name.clone())
    };
    let form = |variant: &Variant, form: &str| variant.form.as_deref() == Some(form);
    let dark = best(&|variant| variant.target && form(variant, "unplated"))
        .or_else(|| best(&|variant| variant.target && variant.form.is_none()))
        .or_else(|| best(&|variant| !variant.target && variant.form.is_none()))
        .or_else(|| {
            files
                .iter()
                .find(|name| name.eq_ignore_ascii_case(logo))
                .cloned()
        })?;
    let light = best(&|variant| variant.target && form(variant, "lightunplated"));
    Some(match light {
        Some(light) => (dark, Some(light)),
        None => (dark, None),
    })
}

/// `name` as a variant of the logo `stem`.`extension`, when it is one.
fn variant(name: &str, stem: &str, extension: &str, base_size: u32) -> Option<Variant> {
    let (name_stem, name_extension) = name.rsplit_once('.')?;
    if !name_extension.eq_ignore_ascii_case(extension) {
        return None;
    }
    let qualifiers = name_stem
        .get(..stem.len())
        .filter(|start| start.eq_ignore_ascii_case(stem))
        .map(|_| &name_stem[stem.len()..])?;
    let qualifiers = qualifiers.strip_prefix('.')?;
    let mut variant = Variant {
        name: name.to_owned(),
        size: 0,
        target: false,
        form: None,
    };
    let mut scale = None;
    for qualifier in qualifiers.split('_') {
        let (key, value) = qualifier.split_once('-')?;
        match key.to_ascii_lowercase().as_str() {
            "targetsize" => {
                variant.size = value.parse().ok()?;
                variant.target = true;
            }
            "scale" => scale = Some(value.parse::<u32>().ok()?),
            "altform" => variant.form = Some(value.to_ascii_lowercase()),
            // High contrast is for that mode only.
            "contrast" => return None,
            _ => {}
        }
    }
    if !variant.target {
        variant.size = base_size * scale.unwrap_or(100) / 100;
    }
    Some(variant)
}

/// The package family and app id of `aumid`, an AppUserModelID
/// (`Microsoft.WindowsCalculator_8wekyb3d8bbwe!App`).
pub fn split_app_user_model_id(aumid: &str) -> Option<(&str, &str)> {
    let (family, app) = aumid.split_once('!')?;
    (!family.is_empty() && !app.is_empty()).then_some((family, app))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
         xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10">
  <Properties>
    <DisplayName>ms-resource:AppStoreName</DisplayName>
    <Logo>Assets\StoreLogo.png</Logo>
  </Properties>
  <Applications>
    <Application Id="Helper" Executable="Helper.exe">
      <uap:VisualElements DisplayName="Helper" Square150x150Logo="Assets\Helper150.png"
        Square44x44Logo="Assets\Helper44.png" />
    </Application>
    <Application Id="App" Executable="Calculator.exe" EntryPoint="Calculator.App">
      <uap:VisualElements DisplayName="ms-resource:AppName"
        Square150x150Logo="Assets\CalculatorMedTile.png"
        Square44x44Logo="Assets\CalculatorAppList.png"
        BackgroundColor="transparent">
      </uap:VisualElements>
    </Application>
    <Application Id="NoList">
      <uap:VisualElements Square150x150Logo="Assets\Tile &amp; More.png" />
    </Application>
  </Applications>
</Package>"#;

    #[test]
    fn the_manifest_names_each_apps_list_logo() {
        assert_eq!(
            manifest_logo(MANIFEST, "App").as_deref(),
            Some(r"Assets\CalculatorAppList.png")
        );
        assert_eq!(
            manifest_logo(MANIFEST, "helper").as_deref(),
            Some(r"Assets\Helper44.png")
        );
        // Without a list logo, the tile's; entities decoded.
        assert_eq!(
            manifest_logo(MANIFEST, "NoList").as_deref(),
            Some(r"Assets\Tile & More.png")
        );
        assert_eq!(manifest_logo(MANIFEST, "Missing"), None);
        assert_eq!(manifest_logo("not xml", "App"), None);
    }

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn the_unplated_target_size_closest_to_256_is_drawn_with_its_light_variant() {
        let files = names(&[
            "CalculatorAppList.scale-100.png",
            "CalculatorAppList.scale-200.png",
            "CalculatorAppList.targetsize-16.png",
            "CalculatorAppList.targetsize-256.png",
            "CalculatorAppList.targetsize-48_altform-unplated.png",
            "CalculatorAppList.targetsize-256_altform-unplated.png",
            "CalculatorAppList.targetsize-256_altform-lightunplated.png",
            "CalculatorAppList.targetsize-256_altform-unplated_contrast-black.png",
            "CalculatorMedTile.scale-200.png",
            "Other.targetsize-256.png",
        ]);
        assert_eq!(
            pick_logos("CalculatorAppList.png", &files),
            Some((
                "CalculatorAppList.targetsize-256_altform-unplated.png".into(),
                Some("CalculatorAppList.targetsize-256_altform-lightunplated.png".into())
            ))
        );
    }

    #[test]
    fn without_unplated_variants_a_plated_size_then_a_scale_then_the_file_serves_both_themes() {
        let plated = names(&[
            "Logo.targetsize-24.png",
            "Logo.targetsize-32.png",
            "Logo.scale-400.png",
        ]);
        assert_eq!(
            pick_logos("Logo.png", &plated),
            Some(("Logo.targetsize-32.png".into(), None))
        );
        let scales = names(&[
            "Logo.scale-100.png",
            "Logo.scale-400.png",
            "Logo.scale-200.png",
        ]);
        assert_eq!(
            pick_logos("Logo.png", &scales),
            Some(("Logo.scale-400.png".into(), None))
        );
        assert_eq!(
            pick_logos("Logo.png", &names(&["logo.png", "Readme.txt"])),
            Some(("logo.png".into(), None))
        );
        assert_eq!(pick_logos("Logo.png", &names(&["Other.png"])), None);
        // Larger than wanted beats smaller.
        let sizes = names(&[
            "Logo.targetsize-200_altform-unplated.png",
            "Logo.targetsize-512_altform-unplated.png",
        ]);
        assert_eq!(
            pick_logos("Logo.png", &sizes),
            Some(("Logo.targetsize-512_altform-unplated.png".into(), None))
        );
    }

    #[test]
    fn an_app_user_model_id_is_a_family_and_an_app() {
        assert_eq!(
            split_app_user_model_id("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"),
            Some(("Microsoft.WindowsCalculator_8wekyb3d8bbwe", "App"))
        );
        assert_eq!(split_app_user_model_id("NoApp!"), None);
        assert_eq!(split_app_user_model_id("Vendor.Tool"), None);
    }
}
