//! Resolution subsystem for indirect call analysis.
//!
//! This module provides specialized resolvers for different types of indirect function calls:
//! - Function pointers (fnptr)
//! - Dynamic trait objects (dyn_trait)
//! - Address-taken function analysis (address_taken)
//! - Common utilities (helpers)

pub(crate) mod address_taken;
pub(crate) mod dyn_trait;
pub(crate) mod fnptr;
pub(crate) mod helpers;

pub(crate) use address_taken::{build_fn_sig_index, collect_address_taken_functions};
pub(crate) use dyn_trait::{
    candidates_for_dyn_fn_trait, candidates_for_dyn_normal_trait, extract_dyn_fn_signature, extract_dyn_trait_info,
    peel_dyn_from_receiver,
};
pub(crate) use fnptr::candidates_for_fnptr_sig;
pub(crate) use helpers::{fallback_callable_def_id_from_ty, monomorphize, operand_fn_def, trivial_resolve};
