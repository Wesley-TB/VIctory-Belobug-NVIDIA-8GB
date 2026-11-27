//! The `VICTORY_*` on/off switches (`VICTORY_D3D12_DEBUG=1` and its siblings).

use std::ffi::OsStr;

/// Whether a switch's value turns it on: it is set, and is not `0`.
///
/// A switch used to be on whenever its variable existed, so `VICTORY_D3D12_WARP=0` (typed to switch it off, or left
/// over from comparing two runs) still selected the software adapter. Any other value keeps the old meaning.
fn value_is_on(value: Option<&OsStr>) -> bool {
    value.is_some_and(|v| v != "0")
}

/// Whether the environment variable `name` switches something on.
pub(crate) fn on(name: &str) -> bool {
    value_is_on(std::env::var_os(name).as_deref())
}

/// A switch whose default the host GPU decides (`crate::host::profile`).
/// When its variable is set, `read` of the value decides, so each switch
/// keeps its own reading (`=1` turns it on, or anything but `0` does).
/// Unset, it is `nvidia` on an NVIDIA host and `default` on every other: an
/// AMD host, or one whose vendor is not known, keeps the default it had.
/// Makes the host device if it is not made yet; read it once per switch
/// (in its `OnceLock`), never while the device is being made.
#[cfg(windows)]
#[expect(dead_code, reason = "the NVIDIA defaults (fallback heaps, residency waits, card buffers) are next")]
pub(crate) fn by_host(name: &str, read: impl FnOnce(&OsStr) -> bool, default: bool, nvidia: bool) -> bool {
    let set = std::env::var_os(name).map(|v| read(&v));
    pick(set, crate::host::nvidia_host(), default, nvidia)
}

/// [`by_host`]'s choice: what the variable says, else the host's default.
#[cfg_attr(not(test), expect(dead_code, reason = "used by `by_host`, whose callers are next"))]
fn pick(set: Option<bool>, nvidia_host: bool, default: bool, nvidia: bool) -> bool {
    set.unwrap_or(if nvidia_host { nvidia } else { default })
}

/// A switch that is on unless its variable is `0`, read once; with
/// `VICTORY_AB` naming it (diagnostics builds, `crate::ab`), on and off while
/// the title runs.
#[cfg(windows)]
#[macro_export]
#[doc(hidden)]
macro_rules! toggled {
    ($name:literal) => {{
        #[cfg(feature = "diagnostics")]
        {
            static BIT: std::sync::OnceLock<Result<bool, u32>> = std::sync::OnceLock::new();
            match BIT.get_or_init(|| match $crate::ab::bit($name) {
                Some(bit) => Err(bit),
                None => Ok(std::env::var_os($name).is_none_or(|v| v != "0")),
            }) {
                Ok(on) => *on,
                Err(bit) => $crate::ab::on(*bit),
            }
        }
        #[cfg(not(feature = "diagnostics"))]
        {
            static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
            *ON.get_or_init(|| std::env::var_os($name).is_none_or(|v| v != "0"))
        }
    }};
}

#[cfg(test)]
mod tests {
    use super::{pick, value_is_on};
    use std::ffi::OsStr;

    #[test]
    fn a_host_default_keeps_the_old_default_off_nvidia() {
        // Not NVIDIA (AMD, or not known): the default as it was, either way.
        for default in [false, true] {
            assert_eq!(pick(None, false, default, !default), default);
        }
    }

    #[test]
    fn a_host_default_is_the_nvidia_one_on_nvidia() {
        assert!(pick(None, true, false, true));
        assert!(!pick(None, true, true, false));
    }

    #[test]
    fn a_set_variable_wins_on_every_host() {
        for nvidia_host in [false, true] {
            assert!(pick(Some(true), nvidia_host, false, false));
            assert!(!pick(Some(false), nvidia_host, true, true));
        }
    }

    #[test]
    fn unset_is_off() {
        assert!(!value_is_on(None));
    }

    #[test]
    fn zero_is_off() {
        assert!(!value_is_on(Some(OsStr::new("0"))));
    }

    #[test]
    fn one_is_on() {
        assert!(value_is_on(Some(OsStr::new("1"))));
    }

    #[test]
    fn other_values_keep_their_old_meaning() {
        // Documented as `=1`; anything else that was on before stays on: only `0` is new.
        for value in ["", "true", "yes", "2", "00", " 0", "0 "] {
            assert!(value_is_on(Some(OsStr::new(value))), "{value:?}");
        }
    }
}
