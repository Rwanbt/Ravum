//! Fixture COLLAB - unite de patch contenant deux fonctions independantes.
//!
//! Sert a demontrer la collaboration COLLAB-1/COLLAB-2 : deux agents modifient
//! deux fonctions distinctes du meme fichier, puis le resultat fusionne est
//! reellement compile et active (COLLAB-4 partiel).
//!
//! Les commentaires sont volontairement en ASCII : les lignes de contexte d'un
//! patch ne doivent pas dependre de l'encodage du terminal ou de l'editeur.

use std::os::raw::c_char;

const MANIFEST: &[u8] = b"{\"schema_version\":1,\"patch_id\":\"collab-v0\",\"export\":\"ravum_compute\",\"signature\":\"f64->f64\",\"author\":\"fixture\"}\0";

#[no_mangle]
pub extern "C" fn ravum_abi_version() -> u32 {
    1
}

#[no_mangle]
pub extern "C" fn ravum_manifest_v1() -> *const c_char {
    MANIFEST.as_ptr() as *const c_char
}

/// Calcule un decalage simple.
fn helper(x: f64) -> f64 {
    x + 1.0
}

/// Point d'entree patchable.
#[no_mangle]
pub extern "C" fn ravum_compute(x: f64) -> f64 {
    helper(x) * 5.0
}
