//! Fixture de latence - v1.
//!
//! Dort uniquement pour l'entree sentinelle 99.0, ce qui permet de tester les
//! appels "en vol" sans ralentir la verification (99.0 n'est pas dans le jeu
//! d'entrees du contrat smoke).

use std::os::raw::c_char;

const MANIFEST: &[u8] = b"{\"schema_version\":1,\"patch_id\":\"slow-v1\",\"export\":\"ravum_compute\",\"signature\":\"f64->f64\"}\0";

#[no_mangle]
pub extern "C" fn ravum_abi_version() -> u32 {
    1
}

#[no_mangle]
pub extern "C" fn ravum_manifest_v1() -> *const c_char {
    MANIFEST.as_ptr() as *const c_char
}

#[no_mangle]
pub extern "C" fn ravum_compute(x: f64) -> f64 {
    if x == 99.0 {
        // 3 s : plus long que la boucle complete (analyse + build + verify),
        // pour que la fenetre "appel en vol" soit fiable et non probabiliste.
        std::thread::sleep(std::time::Duration::from_millis(3000));
    }
    x * 2.0
}
