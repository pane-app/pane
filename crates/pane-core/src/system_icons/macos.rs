//! System icons on macOS: `NSWorkspace`'s icon for the file at a path (an
//! application bundle's own, a document's kind's, a folder's), of which
//! Pane keeps the largest image up to twice the size it asks for, as a PNG.

use std::path::Path;

use objc2::rc::autoreleasepool;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSWorkspace};
use objc2_foundation::{NSArray, NSDictionary, NSString};

use super::{ICON_SIZE, SystemIcon};

pub(super) fn icon(path: &Path) -> Result<SystemIcon, String> {
    let text = path
        .to_str()
        .ok_or_else(|| format!("{} is not a path macOS can name", path.display()))?;
    let none = || format!("macOS has no icon for {}", path.display());
    autoreleasepool(|_| {
        let workspace = NSWorkspace::sharedWorkspace();
        let image = workspace.iconForFile(&NSString::from_str(text));
        let tiff = image.TIFFRepresentation().ok_or_else(none)?;
        let images = NSBitmapImageRep::imageRepsWithData(&tiff);
        let largest = i64::from(ICON_SIZE * 2);
        let mut best = None;
        let mut best_width = 0;
        for index in 0..images.count() {
            let candidate = images.objectAtIndex(index);
            let width = candidate.pixelsWide() as i64;
            // The largest up to twice the size asked for, else the
            // smallest larger one.
            let better = match best {
                None => true,
                Some(_) if best_width > largest => width < best_width,
                Some(_) => width > best_width && width <= largest,
            };
            if better {
                best_width = width;
                best = Some(candidate);
            }
        }
        let best = best.ok_or_else(none)?;
        let chosen = NSArray::from_retained_slice(&[best]);
        let properties = NSDictionary::<NSString, AnyObject>::new();
        // SAFETY: an empty dictionary of properties is of the type asked.
        let png = unsafe {
            NSBitmapImageRep::representationOfImageRepsInArray_usingType_properties(
                &chosen,
                NSBitmapImageFileType::PNG,
                &properties,
            )
        }
        .ok_or_else(none)?;
        Ok(SystemIcon::Png(png.to_vec()))
    })
}
