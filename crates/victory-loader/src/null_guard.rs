//! `VICTORY_NULL_GUARD=1`: a null check the title leaves out (off by default
//! while it is tested).
//!
//! What the title does (game_scarlett_master.exe 1.0.2.1, image offsets):
//! after the start screen's A, a list of start-up callbacks runs
//! (+0x2225bda, one call each at +0x2225c01). One of them, +0x2bb6960, looks
//! an object up by two name hashes (+0x2a23d0, a collection and an item in
//! it) and jumps to +0x1cfa5a0 with the result, which reads +0x38 of it at
//! once. On an NVIDIA RTX 4070 Ti the lookup finds nothing, so every start
//! ends there with a read of 0x38 (7 runs of 7: Series X and S, 4K and
//! 1080p, -Safe or not, `a-publier` at cd8dfc0 and 4eb6cfd).
//!
//! The title's own function just before it (+0x1cfa590) checks its object
//! for null and returns; this gives the callback the same check. Ten bytes
//! of int3 padding before +0x1cfa5a0 become `test rcx, rcx; jz <the ret at
//! +0x1cfa585>; jmp +0x1cfa5a0`, and the callback's tail jump goes there
//! instead. Why the lookup finds nothing is not known yet: this only lets
//! the title go on without the object.
//!
//! With it, the same RTX 4070 Ti goes on past the start screen into the game
//! (Series X profile, 4eb6cfd plus this): about 630 ms a frame at first, while
//! the driver still makes pipelines and the card's 7.6 GB budget is full.

use crate::frame_rate::Patched;
use crate::resolution::Site;

/// The int3 padding before +0x1cfa5a0, after the `ret` at +0x1cfa585.
pub const PADDING: Site = Site {
    at: 0x1cfa586,
    what: "null check before +0x1cfa5a0",
    bytes: &[0xcc; 10],
};

/// The callback's tail jump to +0x1cfa5a0 (`jmp rel32`).
pub const TAIL_JUMP: Site = Site {
    at: 0x2bb697a,
    what: "start-up callback's jump through the null check",
    bytes: &[0xe9, 0x21, 0x3c, 0x14, 0xff],
};

/// The function the callback jumps to, and the `ret` before the padding.
const TARGET: usize = 0x1cfa5a0;
const RET: usize = 0x1cfa585;

/// `test rcx, rcx; jz RET; jmp TARGET`, at [`PADDING`].
pub fn guard_code() -> Vec<u8> {
    let after_jz = PADDING.at + 5;
    let after_jmp = PADDING.at + 7;
    let jz = i8::try_from(RET as isize - after_jz as isize).expect("the ret is near");
    let jmp = i8::try_from(TARGET as isize - after_jmp as isize).expect("the target is near");
    vec![0x48, 0x85, 0xc9, 0x74, jz as u8, 0xeb, jmp as u8]
}

/// The callback's `jmp rel32`, to the guard instead.
pub fn tail_jump_code() -> Vec<u8> {
    let next = TAIL_JUMP.at + 5;
    let rel = i32::try_from(PADDING.at as i64 - next as i64).expect("within the image");
    let mut code = vec![0xe9];
    code.extend_from_slice(&rel.to_le_bytes());
    code
}

pub fn asked() -> bool {
    std::env::var_os("VICTORY_NULL_GUARD").is_some_and(|v| v == "1")
}

/// Applies the guard if asked for; a line in the log and on standard output.
///
/// # Safety
/// The image of `size` bytes at `base` stays mapped and no thread runs its
/// code yet.
#[cfg(windows)]
pub unsafe fn apply_asked(base: usize, size: usize) {
    if !asked() {
        return;
    }
    // The guard first: the jump to it is written only once it is there.
    // SAFETY: the caller's promise.
    let guard = unsafe { crate::resolution::write(base, size, &PADDING, &guard_code()) };
    let jump = if guard == Patched::Done {
        // SAFETY: the caller's promise.
        unsafe { crate::resolution::write(base, size, &TAIL_JUMP, &tail_jump_code()) }
    } else {
        Patched::Failed("the guard was not written")
    };
    for (site, done) in [(&PADDING, guard), (&TAIL_JUMP, jump)] {
        let offset = format!("{:#x}", site.at);
        match done {
            Patched::Done => tracing::info!(site = %offset, what = site.what, "null guard: patched"),
            Patched::Differs => tracing::warn!(site = %offset, what = site.what, "null guard: not patched, the title's bytes there are not the ones expected (another build?)"),
            Patched::Failed(why) => tracing::warn!(site = %offset, what = site.what, why, "null guard: not patched"),
        }
        println!("null guard: {} at title+{offset}: {done:?}", site.what);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guard_returns_or_goes_on() {
        let c = guard_code();
        assert!(c.len() <= PADDING.bytes.len());
        // jz lands on the ret, jmp on the function
        assert_eq!(PADDING.at as isize + 5 + c[4] as i8 as isize, RET as isize);
        assert_eq!(PADDING.at as isize + 7 + c[6] as i8 as isize, TARGET as isize);
    }

    #[test]
    fn the_tail_jump_went_to_the_function_and_now_goes_to_the_guard() {
        let was = i32::from_le_bytes(TAIL_JUMP.bytes[1..5].try_into().unwrap());
        assert_eq!(TAIL_JUMP.at as i64 + 5 + i64::from(was), TARGET as i64);
        let c = tail_jump_code();
        assert_eq!(c.len(), TAIL_JUMP.bytes.len());
        let now = i32::from_le_bytes(c[1..5].try_into().unwrap());
        assert_eq!(TAIL_JUMP.at as i64 + 5 + i64::from(now), PADDING.at as i64);
    }
}
