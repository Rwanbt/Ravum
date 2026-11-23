//! DET-1 / DET-9: the forbidden effects of the two deterministic profiles,
//! seen through the public API (`det` refuses external inputs; the internal
//! record/replay profile only native code).

use ravum_lang_sema::facts::{Effect, Profile};

#[test]
fn det_profiles_forbid_what_they_cannot_replay() {
    let det = Profile::parse("det").expect("det is a profile");
    let refused = det.forbidden().expect("det refuses effects");
    for e in [Effect::Fs, Effect::Net, Effect::Unsafe, Effect::Ffi] {
        assert!(refused.contains(e), "{e:?}");
    }
    assert!(!refused.contains(Effect::Nondet));
    let det_io = Profile::DetIo.forbidden().expect("det io refuses effects");
    assert!(det_io.contains(Effect::Unsafe) && det_io.contains(Effect::Ffi));
    assert!(!det_io.contains(Effect::Net) && !det_io.contains(Effect::Fs));
}
