//! pyo3 bindings exposing [`GuiCore`] to the PyQt6 front-end.
//!
//! The FFI surface is intentionally tiny: three fire-and-forget commands and
//! three snapshot getters. All BLE/OSC work stays in the Rust worker thread.

use std::sync::Arc;

use pyo3::prelude::*;

use crate::config::Config;
use crate::gui::{BridgeStatus as CoreStatus, GuiCore};

/// Immutable snapshot of bridge state, mirrored for Python.
///
/// Fields are exposed as read-only attributes; `connection_label` is computed
/// on the Rust side so the wording stays in one place.
#[pyclass(name = "BridgeStatus", from_py_object)]
#[derive(Clone)]
pub struct PyBridgeStatus {
    #[pyo3(get)]
    pub adapter_ready: bool,
    #[pyo3(get)]
    pub connected: bool,
    #[pyo3(get)]
    pub connecting: bool,
    #[pyo3(get)]
    pub scanning: bool,
    #[pyo3(get)]
    pub camera_address: String,
    #[pyo3(get)]
    pub connection_label: String,
    #[pyo3(get)]
    pub osc_target: String,
}

impl From<CoreStatus> for PyBridgeStatus {
    fn from(s: CoreStatus) -> Self {
        let connection_label = s.connection_label().to_string();
        Self {
            adapter_ready: s.adapter_ready,
            connected: s.connected,
            connecting: s.connecting,
            scanning: s.scanning,
            camera_address: s.camera_address,
            connection_label,
            osc_target: s.osc_target,
        }
    }
}

#[pymethods]
impl PyBridgeStatus {
    fn __repr__(&self) -> String {
        format!(
            "BridgeStatus(adapter_ready={}, connected={}, connecting={}, scanning={}, \
camera_address={:?}, osc_target={:?})",
self.adapter_ready,
self.connected,
self.connecting,
self.scanning,
self.camera_address,
self.osc_target,
        )
    }
}

/// Handle to the Rust bridge worker, owned by the Python process.
///
/// Construction spawns the worker thread; dropping the handle detaches it.
/// The embedded runtime exists solely to drive the `tokio::sync::Mutex`
/// acquisitions in `GuiCore`'s getters — those critical sections are
/// clone-and-return, so blocking here is measured in microseconds.
#[pyclass(name = "CamlinkCore")]
pub struct PyCamlinkCore {
    core: Arc<GuiCore>,
    rt: tokio::runtime::Runtime,
}

#[pymethods]
impl PyCamlinkCore {
    #[new]
    #[pyo3(signature = (config_path=None))]
    fn new(config_path: Option<String>) -> PyResult<Self> {
        // Route Rust `log::info!` output to stderr alongside Python's own.
        let _ = env_logger::try_init();

        let path = match config_path {
            Some(p) => std::path::PathBuf::from(p),
            None => std::env::var("CAMLINK_CONFIG")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                .map(|h| h.join(".config/camlink/config.json"))
                .unwrap_or_default()
            }),
        };
        let config = Config::load(&path).unwrap_or_default();

        let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

        Ok(Self {
            core: Arc::new(GuiCore::new(config)),
           rt,
        })
    }

    /// Kick off a 5-second BLE scan. Returns immediately.
    fn scan(&self) {
        self.core.scan();
    }

    /// Queue a connection attempt. `mac` accepts hyphen or colon separators.
    fn connect(&self, mac: &str) {
        self.core.connect(mac);
    }

    /// Tear down the active session, if any.
    fn disconnect(&self) {
        self.core.disconnect();
    }

    fn status(&self) -> PyBridgeStatus {
        PyBridgeStatus::from(self.rt.block_on(self.core.status()))
    }

    fn diagnostics(&self) -> Vec<String> {
        self.rt.block_on(self.core.diagnostics())
    }

    fn devices(&self) -> Vec<(String, String)> {
        self.rt.block_on(self.core.devices())
    }
}

#[pymodule]
fn blackmagic_camera_bridge(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyCamlinkCore>()?;
    m.add_class::<PyBridgeStatus>()?;
    Ok(())
}
