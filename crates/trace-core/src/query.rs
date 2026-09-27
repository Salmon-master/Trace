//! High-level, read-only queries over the KiCad connectivity graph.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::graph::{Component, ConnectivityGraph};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum QueryError {
    #[error("component {0} was not found")]
    ComponentNotFound(String),
    #[error("net {0} was not found")]
    NetNotFound(String),
    #[error("graph contained an invalid pin reference")]
    InvalidPin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectedPin {
    pub reference: String,
    pub number: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentPin {
    pub number: String,
    pub name: Option<String>,
    pub nets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentPinout {
    pub component: Component,
    pub pins: Vec<ComponentPin>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalContext {
    pub name: String,
    pub pins: Vec<ConnectedPin>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorInput {
    pub sensor: ConnectedPin,
    pub series_component: Option<ConnectedPin>,
    pub target: ConnectedPin,
    pub sensor_net: String,
    pub target_net: String,
    pub adc_channel: Option<String>,
}

pub fn component_pinout(
    graph: &ConnectivityGraph,
    reference: &str,
) -> Result<ComponentPinout, QueryError> {
    let component_id = component_id(graph, reference)?;
    let component = graph.components[component_id].clone();
    let mut pins = Vec::new();

    for net in &graph.nets {
        for pin_id in &net.pins {
            let pin = graph.pins.get(*pin_id).ok_or(QueryError::InvalidPin)?;
            if pin.component != component_id {
                continue;
            }

            let Some(existing) = pins
                .iter_mut()
                .find(|existing: &&mut ComponentPin| existing.number == pin.number)
            else {
                pins.push(ComponentPin {
                    number: pin.number.clone(),
                    name: pin.name.clone(),
                    nets: Vec::new(),
                });
                let last = pins.len() - 1;
                if let Some(net_name) = &net.name {
                    pins[last].nets.push(net_name.clone());
                }
                continue;
            };

            if let Some(net_name) = &net.name {
                if !existing.nets.contains(net_name) {
                    existing.nets.push(net_name.clone());
                }
            }
        }
    }

    pins.sort_by(|left, right| pin_order(&left.number).cmp(&pin_order(&right.number)));
    Ok(ComponentPinout { component, pins })
}

pub fn signal(graph: &ConnectivityGraph, name: &str) -> Result<SignalContext, QueryError> {
    let net = graph
        .nets
        .iter()
        .find(|net| net.name.as_deref() == Some(name))
        .ok_or_else(|| QueryError::NetNotFound(name.to_string()))?;
    let mut pins = Vec::new();
    for pin_id in &net.pins {
        let pin = graph.pins.get(*pin_id).ok_or(QueryError::InvalidPin)?;
        let component = graph
            .components
            .get(pin.component)
            .ok_or(QueryError::InvalidPin)?;
        pins.push(ConnectedPin {
            reference: component.reference.clone(),
            number: pin.number.clone(),
            name: pin.name.clone(),
        });
    }
    Ok(SignalContext {
        name: name.to_string(),
        pins,
    })
}

pub fn sensor_inputs(
    graph: &ConnectivityGraph,
    target_reference: &str,
) -> Result<Vec<SensorInput>, QueryError> {
    let target_id = component_id(graph, target_reference)?;
    let component_nets = component_nets(graph)?;
    let sensor_ids = graph
        .components
        .iter()
        .enumerate()
        .filter(|(_, component)| {
            let text = format!("{} {}", component.reference, component.value).to_ascii_lowercase();
            text.contains("sensor")
        })
        .map(|(id, _)| id)
        .collect::<Vec<_>>();

    let resistor_ids = graph
        .components
        .iter()
        .enumerate()
        .filter(|(_, component)| component.reference.to_ascii_lowercase().starts_with('r'))
        .map(|(id, _)| id)
        .collect::<Vec<_>>();

    let mut results = Vec::new();
    for sensor_id in sensor_ids {
        for &(sensor_net_id, sensor_pin_id) in &component_nets[sensor_id] {
            let sensor_pin = graph
                .pins
                .get(sensor_pin_id)
                .ok_or(QueryError::InvalidPin)?;
            let Some(sensor_net_name) = graph.nets[sensor_net_id].name.as_deref() else {
                continue;
            };

            for &resistor_id in &resistor_ids {
                let resistor_pins = &component_nets[resistor_id];
                let Some(&(resistor_sensor_net, resistor_sensor_pin)) = resistor_pins
                    .iter()
                    .find(|(net_id, _)| *net_id == sensor_net_id)
                else {
                    continue;
                };
                let Some(&(target_net_id, _resistor_target_pin_id)) = resistor_pins
                    .iter()
                    .find(|(net_id, _)| *net_id != resistor_sensor_net)
                else {
                    continue;
                };

                let Some(target_pin_id) =
                    graph.nets[target_net_id]
                        .pins
                        .iter()
                        .copied()
                        .find(|pin_id| {
                            graph
                                .pins
                                .get(*pin_id)
                                .is_some_and(|pin| pin.component == target_id)
                        })
                else {
                    continue;
                };
                let target_pin = graph
                    .pins
                    .get(target_pin_id)
                    .ok_or(QueryError::InvalidPin)?;
                let resistor_pin = graph
                    .pins
                    .get(resistor_sensor_pin)
                    .ok_or(QueryError::InvalidPin)?;
                let target_net_name = graph.nets[target_net_id]
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("net-{target_net_id}"));
                let target_component = graph
                    .components
                    .get(target_id)
                    .ok_or(QueryError::InvalidPin)?;
                let target_name = target_pin
                    .name
                    .clone()
                    .or_else(|| infer_pin_name(&target_net_name, &target_component.reference));
                let adc_channel = target_name.as_deref().and_then(adc_channel);
                if adc_channel.is_none() {
                    continue;
                }

                results.push(SensorInput {
                    sensor: ConnectedPin {
                        reference: graph.components[sensor_id].reference.clone(),
                        number: sensor_pin.number.clone(),
                        name: sensor_pin.name.clone(),
                    },
                    series_component: Some(ConnectedPin {
                        reference: graph.components[resistor_id].reference.clone(),
                        number: resistor_pin.number.clone(),
                        name: resistor_pin.name.clone(),
                    }),
                    target: ConnectedPin {
                        reference: target_component.reference.clone(),
                        number: target_pin.number.clone(),
                        name: target_name,
                    },
                    sensor_net: sensor_net_name.to_string(),
                    target_net: target_net_name,
                    adc_channel,
                });
            }
        }
    }

    results.sort_by(|left, right| {
        pin_order(&left.target.number).cmp(&pin_order(&right.target.number))
    });
    results.dedup_by(|left, right| {
        left.sensor.reference == right.sensor.reference
            && left.sensor.number == right.sensor.number
            && left.target.number == right.target.number
    });
    Ok(results)
}

fn component_id(graph: &ConnectivityGraph, reference: &str) -> Result<usize, QueryError> {
    graph
        .components
        .iter()
        .position(|component| component.reference.eq_ignore_ascii_case(reference))
        .ok_or_else(|| QueryError::ComponentNotFound(reference.to_string()))
}

fn component_nets(graph: &ConnectivityGraph) -> Result<Vec<Vec<(usize, usize)>>, QueryError> {
    let mut result = vec![Vec::new(); graph.components.len()];
    for (net_id, net) in graph.nets.iter().enumerate() {
        for pin_id in &net.pins {
            let pin = graph.pins.get(*pin_id).ok_or(QueryError::InvalidPin)?;
            let Some(component_nets) = result.get_mut(pin.component) else {
                return Err(QueryError::InvalidPin);
            };
            component_nets.push((net_id, *pin_id));
        }
    }
    Ok(result)
}

fn pin_order(number: &str) -> (u32, String) {
    (number.parse().unwrap_or(u32::MAX), number.to_string())
}

fn adc_channel(name: &str) -> Option<String> {
    let upper = name.to_ascii_uppercase();
    let suffix = upper.strip_prefix("PD")?;
    if suffix.len() == 1 && suffix.chars().all(|character| character.is_ascii_digit()) {
        return Some(format!("ADC0.AIN{suffix}"));
    }
    None
}

fn infer_pin_name(net_name: &str, reference: &str) -> Option<String> {
    let prefix = format!("Net-({reference}-");
    let suffix = net_name.strip_prefix(&prefix)?.strip_suffix(')')?;
    suffix.split('/').next().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Component, Net, Pin};

    fn graph() -> ConnectivityGraph {
        ConnectivityGraph {
            components: vec![
                Component {
                    reference: "U1".to_string(),
                    value: "MCU".to_string(),
                    footprint: None,
                    manufacturer: None,
                    mpn: None,
                    datasheet_url: None,
                },
                Component {
                    reference: "R1".to_string(),
                    value: "10k".to_string(),
                    footprint: None,
                    manufacturer: None,
                    mpn: None,
                    datasheet_url: None,
                },
                Component {
                    reference: "sensor_header1".to_string(),
                    value: "Sensor Board".to_string(),
                    footprint: None,
                    manufacturer: None,
                    mpn: None,
                    datasheet_url: None,
                },
            ],
            pins: vec![
                Pin {
                    component: 1,
                    number: "1".to_string(),
                    name: None,
                },
                Pin {
                    component: 2,
                    number: "3".to_string(),
                    name: None,
                },
                Pin {
                    component: 0,
                    number: "10".to_string(),
                    name: Some("PD0".to_string()),
                },
                Pin {
                    component: 1,
                    number: "2".to_string(),
                    name: None,
                },
            ],
            nets: vec![
                Net {
                    name: Some("sensor-net".to_string()),
                    pins: vec![0, 1],
                },
                Net {
                    name: Some("mcu-net".to_string()),
                    pins: vec![2, 3],
                },
            ],
        }
    }

    #[test]
    fn finds_component_pinout() {
        let result = component_pinout(&graph(), "u1").expect("component exists");
        assert_eq!(result.pins[0].name.as_deref(), Some("PD0"));
        assert_eq!(result.pins[0].nets, ["mcu-net"]);
    }

    #[test]
    fn finds_sensor_input_through_resistor() {
        let result = sensor_inputs(&graph(), "U1").expect("query succeeds");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].sensor.reference, "sensor_header1");
        assert_eq!(result[0].target.name.as_deref(), Some("PD0"));
        assert_eq!(result[0].adc_channel.as_deref(), Some("ADC0.AIN0"));
    }
}
