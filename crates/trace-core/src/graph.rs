use serde::{Deserialize, Serialize};

pub type ComponentId = usize;
pub type PinId = usize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Component {
    pub reference: String,
    pub value: String,
    pub footprint: Option<String>,
    pub manufacturer: Option<String>,
    pub mpn: Option<String>,
    pub datasheet_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub component: ComponentId,
    pub number: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Net {
    pub name: Option<String>,
    pub pins: Vec<PinId>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectivityGraph {
    pub components: Vec<Component>,
    pub pins: Vec<Pin>,
    pub nets: Vec<Net>,
}
