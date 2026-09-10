//! Backend auto-detection and probing

use crate::backends::{BackendType, RenderBackend};
use crate::renderer::RenderContext;
use crate::utils::RenderError;

#[cfg(feature = "nostd")]
use alloc::{boxed::Box, vec::Vec};
#[cfg(not(feature = "nostd"))]
use std::vec::Vec;

/// Backend prober for automatic backend selection
pub struct BackendProber {
    preferred_order: Vec<BackendType>,
}

impl BackendProber {
    /// Create a new backend prober with default preferences
    #[allow(clippy::vec_init_then_push)] // Conditional compilation makes vec! macro difficult
    #[allow(unused_mut)] // mut is needed for conditional features
    pub fn new() -> Self {
        let mut preferred_order = Vec::new();

        #[cfg(all(feature = "repose-backend", not(feature = "nostd")))]
        preferred_order.push(BackendType::Repose);

        #[cfg(feature = "software-backend")]
        preferred_order.push(BackendType::Software);

        Self { preferred_order }
    }

    /// Create prober with custom backend preference order
    pub fn with_preference(preferred_order: Vec<BackendType>) -> Self {
        Self { preferred_order }
    }

    /// Probe for the best available backend
    pub fn probe_best_backend(
        &self,
        context: &RenderContext,
    ) -> Result<Box<dyn RenderBackend>, RenderError> {
        for backend_type in &self.preferred_order {
            match self.try_create_backend(*backend_type, context) {
                Ok(backend) => {
                    return Ok(backend);
                }
                Err(_e) => {}
            }
        }

        Err(RenderError::NoBackendAvailable)
    }

    /// Try to create a specific backend
    fn try_create_backend(
        &self,
        backend_type: BackendType,
        context: &RenderContext,
    ) -> Result<Box<dyn RenderBackend>, RenderError> {
        match backend_type {
            #[cfg(feature = "software-backend")]
            BackendType::Software => {
                use crate::backends::software::SoftwareBackend;
                Ok(Box::new(SoftwareBackend::new(context)?))
            }

            #[cfg(all(feature = "repose-backend", not(feature = "nostd")))]
            BackendType::Repose => {
                use crate::backends::repose::ReposeBackend;
                Ok(Box::new(ReposeBackend::new(context)))
            }

            _ => Err(RenderError::UnsupportedBackend(backend_type.as_str())),
        }
    }

    /// Check if a backend is available
    pub fn is_backend_available(&self, backend_type: BackendType) -> bool {
        match backend_type {
            #[cfg(feature = "software-backend")]
            BackendType::Software => true,

            // Construction is lazy (no GPU needed); rendering fails loudly
            // without a WGPU adapter. Unavailable under `nostd`.
            #[cfg(all(feature = "repose-backend", not(feature = "nostd")))]
            BackendType::Repose => true,

            _ => false,
        }
    }

    /// Get list of available backends
    pub fn available_backends(&self) -> Vec<BackendType> {
        self.preferred_order
            .iter()
            .filter(|&&backend| self.is_backend_available(backend))
            .copied()
            .collect()
    }
}

impl Default for BackendProber {
    fn default() -> Self {
        Self::new()
    }
}
