//! Core library for Trace's hardware-context model.

pub mod cache;
pub mod config;
pub mod datasheet;
pub mod graph;
pub mod kicad;
pub mod project;

pub use config::{HardwareConfig, KicadConfig};
pub use datasheet::{DatasheetDocument, DatasheetError, DatasheetMetadata};
pub use graph::{Component, ComponentId, ConnectivityGraph, Net, Pin, PinId};
