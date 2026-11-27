//! `VICTORY_LOCALE=<name>` / `VICTORY_GEO=<code>`: the user's locale name
//! (`en-US`) and geographic region (`US`) the title is told, in place of the
//! host's (off by default: the title asks Windows itself).
//!
//! The title reads both through `GetUserDefaultLocaleName` and
//! `GetUserDefaultGeoName` and picks its region from them (its crash log's
//! `Game Locale`). This lets a run see whether a start depends on the region
//! the PC is set to.
#![allow(non_snake_case)]

use std::sync::LazyLock;

use super::files::host;
use crate::{HleModule, hle_export};

fn wide(var: &str) -> Option<Vec<u16>> {
    let value = std::env::var(var).ok()?;
    let value = value.trim();
    (!value.is_empty() && value.len() <= 84).then(|| value.encode_utf16().chain([0]).collect())
}

static LOCALE: LazyLock<Option<Vec<u16>>> = LazyLock::new(|| wide("VICTORY_LOCALE"));
static GEO: LazyLock<Option<Vec<u16>>> = LazyLock::new(|| wide("VICTORY_GEO"));

/// Whether either is asked for (the module is served only then).
pub fn serving() -> bool {
    LOCALE.is_some() || GEO.is_some()
}

/// Copies `name` (with its NUL) into `out` of `count` units: the units
/// written, or the units needed when `count` is 0, or 0 if it is too small.
fn copy(name: &[u16], out: *mut u16, count: i32) -> i32 {
    let need = name.len() as i32;
    if count == 0 {
        return need;
    }
    if out.is_null() || count < need {
        return 0;
    }
    // SAFETY: the caller's buffer holds `count` units, at least `need`.
    unsafe { std::ptr::copy_nonoverlapping(name.as_ptr(), out, name.len()) };
    need
}

pub extern "system" fn GetUserDefaultLocaleName(out: *mut u16, count: i32) -> i32 {
    match &*LOCALE {
        // (this call has no sizing form: too small is a failure)
        Some(name) if count > 0 => copy(name, out, count),
        Some(_) => 0,
        None => host!("kernel32.dll", "GetUserDefaultLocaleName": extern "system" fn(*mut u16, i32) -> i32)(out, count),
    }
}

pub extern "system" fn GetUserDefaultGeoName(out: *mut u16, count: i32) -> i32 {
    match &*GEO {
        Some(name) => copy(name, out, count),
        None => host!("kernel32.dll", "GetUserDefaultGeoName": extern "system" fn(*mut u16, i32) -> i32)(out, count),
    }
}

pub(crate) fn module() -> HleModule {
    if let Some(name) = &*LOCALE {
        tracing::info!(locale = %String::from_utf16_lossy(&name[..name.len() - 1]), "locale: the title is told this locale (VICTORY_LOCALE)");
    }
    if let Some(name) = &*GEO {
        tracing::info!(geo = %String::from_utf16_lossy(&name[..name.len() - 1]), "locale: the title is told this region (VICTORY_GEO)");
    }
    HleModule {
        name: "locale",
        dlls: &["kernel32.dll", "kernelbase.dll", "api-ms-win-core-localization-*"],
        exports: vec![hle_export!(GetUserDefaultLocaleName), hle_export!(GetUserDefaultGeoName)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_copy_with_their_nul_or_report_their_size() {
        let name: Vec<u16> = "en-US\0".encode_utf16().collect();
        assert_eq!(copy(&name, std::ptr::null_mut(), 0), 6);
        let mut small = [0u16; 3];
        assert_eq!(copy(&name, small.as_mut_ptr(), 3), 0);
        let mut buf = [0xffffu16; 85];
        assert_eq!(copy(&name, buf.as_mut_ptr(), 85), 6);
        assert_eq!(&buf[..6], &name[..]);
    }
}
