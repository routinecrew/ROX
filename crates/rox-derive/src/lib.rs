//! # rox-derive
//!
//! Procedural macros for Rox node definitions.
//! Provides `#[rox_node]`, `#[rox_sub]`, `#[rox_pub]`, `#[rox_param]`.

use proc_macro::TokenStream;

/// Derive macro for Rox node definitions.
/// Automatically generates RoxNode trait boilerplate.
#[proc_macro_attribute]
pub fn rox_node(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // TODO: Implement node derivation
    item
}
