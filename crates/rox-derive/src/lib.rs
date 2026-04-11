//! # rox-derive
//!
//! Procedural macros for Rox node definitions.
//! Provides the `#[rox_node]` attribute macro that generates RoxNode metadata boilerplate.
//!
//! ## What it generates
//!
//! Given a struct annotated with `#[rox_node]`, the macro scans for three field attributes:
//!
//! - `#[rox_sub("topic")]` — marks a field as a subscription input
//! - `#[rox_pub("topic")]` — marks a field as a publication output
//! - `#[rox_param(default = value)]` — marks a configurable parameter
//!
//! It then generates an `impl` block with:
//! - `rox_name() -> &'static str` — returns the struct name as a string
//! - `rox_subscriptions() -> &'static [&'static str]` — lists all `#[rox_sub]` topics
//! - `rox_publications() -> &'static [&'static str]` — lists all `#[rox_pub]` topics
//!
//! ## Usage
//!
//! ```ignore
//! use rox_derive::rox_node;
//!
//! #[rox_node]
//! struct VoxelFilter {
//!     #[rox_sub("lidar/raw")]
//!     input: PointCloud,
//!
//!     #[rox_pub("lidar/filtered")]
//!     output: PointCloud,
//!
//!     #[rox_param(default = 0.1)]
//!     voxel_size: f32,
//! }
//!
//! assert_eq!(VoxelFilter::rox_name(), "VoxelFilter");
//! assert_eq!(VoxelFilter::rox_subscriptions(), &["lidar/raw"]);
//! assert_eq!(VoxelFilter::rox_publications(), &["lidar/filtered"]);
//! ```
//!
//! ## Design rationale
//!
//! The macro keeps node definitions declarative, letting the Rox runtime introspect
//! topic dependencies at startup for automatic TaskGraph wiring. This is inspired by
//! copper-rs's `#[copper_runtime]` macro approach.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Field, ItemStruct};

/// Attribute macro for Rox node definitions.
///
/// Generates:
/// - Default `name()` implementation returning the struct name
/// - Default `init()` / `shutdown()` implementations
/// - Field metadata extraction for `#[rox_sub]`, `#[rox_pub]`, `#[rox_param]`
#[proc_macro_attribute]
pub fn rox_node(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemStruct);
    let name = &input.ident;
    let name_str = name.to_string();

    // Extract rox_sub fields
    let sub_fields: Vec<_> = input
        .fields
        .iter()
        .filter(|f| has_attr(f, "rox_sub"))
        .collect();

    // Extract rox_pub fields
    let pub_fields: Vec<_> = input
        .fields
        .iter()
        .filter(|f| has_attr(f, "rox_pub"))
        .collect();

    // Generate subscription topic list
    let sub_topics: Vec<String> = sub_fields
        .iter()
        .filter_map(|f| extract_attr_value(f, "rox_sub"))
        .collect();

    // Generate publication topic list
    let pub_topics: Vec<String> = pub_fields
        .iter()
        .filter_map(|f| extract_attr_value(f, "rox_pub"))
        .collect();

    let sub_topic_strs: Vec<&str> = sub_topics.iter().map(|s| s.as_str()).collect();
    let pub_topic_strs: Vec<&str> = pub_topics.iter().map(|s| s.as_str()).collect();

    // Strip rox_sub, rox_pub, rox_param attributes from the output struct
    let clean_struct = strip_rox_attrs(&input);

    // Generate the augmented struct (cleaned + metadata methods)
    let expanded = quote! {
        #clean_struct

        impl #name {
            /// Node name (auto-generated from struct name).
            pub fn rox_name() -> &'static str {
                #name_str
            }

            /// List of subscription topics declared with `#[rox_sub(...)]`.
            pub fn rox_subscriptions() -> &'static [&'static str] {
                &[#(#sub_topic_strs),*]
            }

            /// List of publication topics declared with `#[rox_pub(...)]`.
            pub fn rox_publications() -> &'static [&'static str] {
                &[#(#pub_topic_strs),*]
            }
        }
    };

    TokenStream::from(expanded)
}

/// Strip rox_sub, rox_pub, rox_param attributes from struct fields.
fn strip_rox_attrs(input: &ItemStruct) -> ItemStruct {
    let mut cleaned = input.clone();
    if let syn::Fields::Named(ref mut fields) = cleaned.fields {
        for field in &mut fields.named {
            field.attrs.retain(|a| {
                !a.path().is_ident("rox_sub")
                    && !a.path().is_ident("rox_pub")
                    && !a.path().is_ident("rox_param")
            });
        }
    }
    cleaned
}

/// Check if a field has a specific attribute.
fn has_attr(field: &Field, attr_name: &str) -> bool {
    field.attrs.iter().any(|a| a.path().is_ident(attr_name))
}

/// Extract the string value from an attribute like `#[rox_sub("topic/name")]`.
fn extract_attr_value(field: &Field, attr_name: &str) -> Option<String> {
    for attr in &field.attrs {
        if attr.path().is_ident(attr_name) {
            // Try to parse as a string literal argument
            if let Ok(lit) = attr.parse_args::<syn::LitStr>() {
                return Some(lit.value());
            }
        }
    }
    None
}
