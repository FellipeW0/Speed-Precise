//! Official application icon loader decoding the root `Speed_Precise.ico` asset
//! for the OS window, titlebar, taskbar, and Alt+Tab switcher (`egui::IconData`).

/// Embedded official application icon bytes from the workspace root (`Speed_Precise.ico`).
pub const APP_ICON_ICO_BYTES: &[u8] = include_bytes!("../../../Speed_Precise.ico");

const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Extracts the largest PNG-encoded icon frame from a Windows `.ico` container.
pub fn extract_largest_png_from_ico(ico_bytes: &[u8]) -> Option<&[u8]> {
    if ico_bytes.len() < 6 {
        return None;
    }
    let reserved = u16::from_le_bytes([ico_bytes[0], ico_bytes[1]]);
    let resource_type = u16::from_le_bytes([ico_bytes[2], ico_bytes[3]]);
    let count = u16::from_le_bytes([ico_bytes[4], ico_bytes[5]]) as usize;
    if reserved != 0 || resource_type != 1 || count == 0 || ico_bytes.len() < 6 + count * 16 {
        return None;
    }

    let mut best: Option<(u32, &[u8])> = None;
    for idx in 0..count {
        let dir_off = 6 + idx * 16;
        let entry = &ico_bytes[dir_off..dir_off + 16];
        let width = if entry[0] == 0 {
            256
        } else {
            u32::from(entry[0])
        };
        let height = if entry[1] == 0 {
            256
        } else {
            u32::from(entry[1])
        };
        let size = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]) as usize;
        let offset = u32::from_le_bytes([entry[12], entry[13], entry[14], entry[15]]) as usize;
        let Some(end) = offset.checked_add(size) else {
            continue;
        };
        if end > ico_bytes.len() {
            continue;
        }
        let slice = &ico_bytes[offset..end];
        if slice.starts_with(&PNG_MAGIC) {
            let area = width.saturating_mul(height);
            if best.is_none_or(|(best_area, _)| area > best_area) {
                best = Some((area, slice));
            }
        }
    }

    best.map(|(_, slice)| slice)
}

/// Decodes `Speed_Precise.ico` into `egui::IconData` (`RGBA8`) for the OS window & taskbar.
pub fn load_app_icon_data() -> Option<egui::IconData> {
    let png_slice = extract_largest_png_from_ico(APP_ICON_ICO_BYTES)?;
    eframe::icon_data::from_png_bytes(png_slice).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_official_app_ico_decodes_cleanly_to_256px_rgba() {
        let png_slice = extract_largest_png_from_ico(APP_ICON_ICO_BYTES)
            .expect("Speed_Precise.ico should contain a valid PNG icon frame");
        assert!(png_slice.starts_with(&PNG_MAGIC));

        let icon = load_app_icon_data().expect("Speed_Precise.ico should decode into IconData");
        assert_eq!(icon.width, 256);
        assert_eq!(icon.height, 256);
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
        assert!(
            icon.rgba.chunks_exact(4).any(|px| px[3] > 0),
            "Decoded app icon must contain visible non-transparent pixels"
        );
    }
}
