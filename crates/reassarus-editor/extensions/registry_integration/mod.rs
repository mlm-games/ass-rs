//! Integration with reassarus-core's ExtensionRegistry
//!
//! This module provides the glue between reassarus-editor's extension system and
//! reassarus-core's plugin system, allowing editor extensions to register tag handlers
//! and section processors that will be used during ASS parsing.

mod adapters;
mod builtins;
mod integration;

#[cfg(test)]
mod tests;

pub use adapters::{EditorSectionProcessorAdapter, EditorTagHandlerAdapter};
pub use integration::RegistryIntegration;
