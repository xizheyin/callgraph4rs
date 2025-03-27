use rustc_hir::def_id::DefId;
use rustc_middle::ty::TyCtxt;

// Get version information for a specific DefId from TyCtxt
pub(crate) fn get_crate_version<'tcx>(tcx: TyCtxt<'tcx>, def_id: DefId) -> String {
    // Get the crate number for this DefId
    let crate_num = def_id.krate;

    // Try to get the crate name
    let crate_name = tcx.crate_name(crate_num);

    // Check if we can get version from crate disambiguator (hash)
    let crate_hash = tcx.crate_hash(crate_num);

    // For built-in crates and std library, we can use the Rust version
    if crate_num == rustc_hir::def_id::LOCAL_CRATE {
        // This is the current crate being analyzed
        // Try to get version from environment if available
        match option_env!("CARGO_PKG_VERSION") {
            Some(version) => return version.to_string(),
            None => {}
        }
    }

    // Look for version patterns in the crate name (some crates include version in name)
    // Format: name-x.y.z
    let crate_name_str = crate_name.to_string();
    if let Some(idx) = crate_name_str.rfind('-') {
        let potential_version = &crate_name_str[idx + 1..];
        if potential_version
            .chars()
            .next()
            .map_or(false, |c| c.is_digit(10))
        {
            return potential_version.to_string();
        }
    }

    // If we can't find a proper version, use the crate hash as a unique identifier
    format!("0.0.0-{}", crate_hash.to_string().split_at(8).0)
}
