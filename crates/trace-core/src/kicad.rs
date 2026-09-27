//! KiCad command-line integration and netlist parsing.

use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::HashMap;
use std::io::{BufRead, Cursor};
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

use crate::{Component, ConnectivityGraph, Net, Pin};

#[derive(Debug, Error)]
pub enum KicadError {
    #[error("kicad-cli was not found; set TRACE_KICAD_CLI or add KiCad to PATH")]
    NotFound,
    #[error("failed to run {executable}: {source}")]
    Run {
        executable: String,
        source: std::io::Error,
    },
    #[error("KiCad netlist export failed with status {status}: {stderr}")]
    ExportFailed { status: String, stderr: String },
    #[error("failed to read KiCad XML {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to parse KiCad XML: {0}")]
    Xml(#[from] quick_xml::Error),
    #[error("failed to decode KiCad XML text: {0}")]
    Encoding(#[from] quick_xml::encoding::EncodingError),
    #[error("netlist node references unknown component {reference}")]
    UnknownComponent { reference: String },
}

#[derive(Debug, Clone)]
pub struct KicadCli {
    executable: PathBuf,
}

impl KicadCli {
    pub fn discover() -> Result<Self, KicadError> {
        if let Ok(value) = std::env::var("TRACE_KICAD_CLI") {
            let path = PathBuf::from(value);
            if path.is_file() {
                return Ok(Self { executable: path });
            }
        }

        if let Some(path) = find_on_path("kicad-cli") {
            return Ok(Self { executable: path });
        }

        #[cfg(windows)]
        for root in windows_program_roots() {
            let kicad_root = root.join("KiCad");
            let Ok(entries) = std::fs::read_dir(&kicad_root) else {
                continue;
            };

            let mut versions: Vec<_> = entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect();
            versions.sort_by(|left, right| right.cmp(left));

            for version in versions {
                let candidate = version.join("bin").join("kicad-cli.exe");
                if candidate.is_file() {
                    return Ok(Self {
                        executable: candidate,
                    });
                }
            }
        }

        Err(KicadError::NotFound)
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub fn export_netlist(
        &self,
        schematic: impl AsRef<Path>,
        output: impl AsRef<Path>,
    ) -> Result<(), KicadError> {
        let schematic = schematic.as_ref();
        let output = output.as_ref();
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent).map_err(|source| KicadError::Read {
                path: parent.display().to_string(),
                source,
            })?;
        }

        let result = Command::new(&self.executable)
            .args([
                "sch", "export", "netlist", "--format", "kicadxml", "--output",
            ])
            .arg(output)
            .arg(schematic)
            .output()
            .map_err(|source| KicadError::Run {
                executable: self.executable.display().to_string(),
                source,
            })?;

        if !result.status.success() {
            return Err(KicadError::ExportFailed {
                status: result
                    .status
                    .code()
                    .map_or_else(|| "unknown".to_string(), |code| code.to_string()),
                stderr: String::from_utf8_lossy(&result.stderr).trim().to_string(),
            });
        }

        Ok(())
    }
}

#[cfg(windows)]
fn windows_program_roots() -> Vec<PathBuf> {
    ["ProgramFiles", "ProgramFiles(x86)"]
        .into_iter()
        .filter_map(|name| std::env::var_os(name).map(PathBuf::from))
        .collect()
}

fn find_on_path(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|directory| {
            let candidate = directory.join(program);
            if candidate.is_file() {
                return Some(candidate);
            }

            #[cfg(windows)]
            {
                let candidate = directory.join(format!("{program}.exe"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }

            None
        })
    })
}

#[derive(Debug, Default)]
struct RawComponent {
    reference: String,
    value: String,
    footprint: Option<String>,
    datasheet_url: Option<String>,
    fields: HashMap<String, String>,
}

#[derive(Debug, Default)]
struct RawNet {
    name: Option<String>,
    nodes: Vec<RawNode>,
}

#[derive(Debug)]
struct RawNode {
    reference: String,
    number: String,
    name: Option<String>,
}

#[derive(Debug)]
enum TextTarget {
    Value,
    Footprint,
    Datasheet,
    Field(String),
}

pub fn parse_kicad_xml(path: impl AsRef<Path>) -> Result<ConnectivityGraph, KicadError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|source| KicadError::Read {
        path: path.display().to_string(),
        source,
    })?;
    parse_kicad_xml_reader(Cursor::new(bytes))
}

fn parse_kicad_xml_reader<R: BufRead>(reader: R) -> Result<ConnectivityGraph, KicadError> {
    let mut reader = Reader::from_reader(reader);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut components = Vec::new();
    let mut nets = Vec::new();
    let mut current_component = None;
    let mut current_net = None;
    let mut text_target = None;

    loop {
        match reader.read_event_into(&mut buffer)? {
            Event::Start(event) => match event.name().as_ref() {
                b"comp" => {
                    current_component = Some(RawComponent {
                        reference: attribute(&event, b"ref").unwrap_or_default(),
                        ..RawComponent::default()
                    });
                }
                b"net" => {
                    current_net = Some(RawNet {
                        name: non_empty(attribute(&event, b"name")),
                        ..RawNet::default()
                    });
                }
                b"node" => {
                    if let Some(net) = current_net.as_mut() {
                        net.nodes.push(RawNode {
                            reference: attribute(&event, b"ref").unwrap_or_default(),
                            number: attribute(&event, b"pin").unwrap_or_default(),
                            name: node_name(&event),
                        });
                    }
                }
                b"value" if current_component.is_some() => {
                    text_target = Some(TextTarget::Value);
                }
                b"footprint" if current_component.is_some() => {
                    text_target = Some(TextTarget::Footprint);
                }
                b"datasheet" if current_component.is_some() => {
                    text_target = Some(TextTarget::Datasheet);
                }
                b"field" if current_component.is_some() => {
                    let name = attribute(&event, b"name").unwrap_or_default();
                    if let Some(value) = attribute(&event, b"value") {
                        if let Some(component) = current_component.as_mut() {
                            component.fields.insert(name.clone(), value);
                        }
                    }
                    text_target = Some(TextTarget::Field(name));
                }
                _ => {}
            },
            Event::Empty(event) => match event.name().as_ref() {
                b"node" => {
                    if let Some(net) = current_net.as_mut() {
                        net.nodes.push(RawNode {
                            reference: attribute(&event, b"ref").unwrap_or_default(),
                            number: attribute(&event, b"pin").unwrap_or_default(),
                            name: node_name(&event),
                        });
                    }
                }
                b"field" if current_component.is_some() => {
                    let name = attribute(&event, b"name").unwrap_or_default();
                    if let Some(value) = attribute(&event, b"value") {
                        if let Some(component) = current_component.as_mut() {
                            component.fields.insert(name, value);
                        }
                    }
                }
                _ => {}
            },
            Event::Text(text) => {
                if let Some(target) = text_target.as_ref() {
                    let value = text.decode()?.into_owned();
                    if let Some(component) = current_component.as_mut() {
                        match target {
                            TextTarget::Value => component.value.push_str(&value),
                            TextTarget::Footprint => component
                                .footprint
                                .get_or_insert_with(String::new)
                                .push_str(&value),
                            TextTarget::Datasheet => component
                                .datasheet_url
                                .get_or_insert_with(String::new)
                                .push_str(&value),
                            TextTarget::Field(name) => {
                                component
                                    .fields
                                    .entry(name.clone())
                                    .or_default()
                                    .push_str(&value);
                            }
                        }
                    }
                }
            }
            Event::CData(text) => {
                if let Some(target) = text_target.as_ref() {
                    let value = text.decode()?.into_owned();
                    if let Some(component) = current_component.as_mut() {
                        match target {
                            TextTarget::Value => component.value.push_str(&value),
                            TextTarget::Footprint => component
                                .footprint
                                .get_or_insert_with(String::new)
                                .push_str(&value),
                            TextTarget::Datasheet => component
                                .datasheet_url
                                .get_or_insert_with(String::new)
                                .push_str(&value),
                            TextTarget::Field(name) => {
                                component
                                    .fields
                                    .entry(name.clone())
                                    .or_default()
                                    .push_str(&value);
                            }
                        }
                    }
                }
            }
            Event::End(event) => match event.name().as_ref() {
                b"value" | b"footprint" | b"datasheet" | b"field" => text_target = None,
                b"comp" => {
                    if let Some(component) = current_component.take() {
                        components.push(component);
                    }
                }
                b"net" => {
                    if let Some(net) = current_net.take() {
                        nets.push(net);
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }

    let mut component_ids = HashMap::new();
    let components = components
        .into_iter()
        .enumerate()
        .map(|(id, component)| {
            component_ids.insert(component.reference.clone(), id);
            Component {
                reference: component.reference,
                value: component.value,
                footprint: non_empty(component.footprint),
                manufacturer: field_value(&component.fields, "manufacturer"),
                mpn: field_value(&component.fields, "mpn"),
                datasheet_url: non_empty(component.datasheet_url),
            }
        })
        .collect::<Vec<_>>();

    let mut pins = Vec::new();
    let mut graph_nets = Vec::new();
    for net in nets {
        let mut pin_ids = Vec::new();
        for node in net.nodes {
            let Some(&component) = component_ids.get(&node.reference) else {
                return Err(KicadError::UnknownComponent {
                    reference: node.reference,
                });
            };
            pin_ids.push(pins.len());
            pins.push(Pin {
                component,
                number: node.number,
                name: node.name,
            });
        }
        graph_nets.push(Net {
            name: net.name,
            pins: pin_ids,
        });
    }

    Ok(ConnectivityGraph {
        components,
        pins,
        nets: graph_nets,
    })
}

fn attribute<'a>(event: &quick_xml::events::BytesStart<'a>, key: &[u8]) -> Option<String> {
    event
        .attributes()
        .with_checks(false)
        .filter_map(Result::ok)
        .find(|attribute| attribute.key.as_ref() == key)
        .and_then(|attribute| attribute.unescape_value().ok())
        .map(|value| value.into_owned())
}

fn node_name(event: &quick_xml::events::BytesStart<'_>) -> Option<String> {
    non_empty(attribute(event, b"name").or_else(|| attribute(event, b"pinfunction")))
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

fn field_value(fields: &HashMap<String, String>, wanted: &str) -> Option<String> {
    fields
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(wanted))
        .and_then(|(_, value)| non_empty(Some(value.clone())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_components_and_named_nets() {
        let xml = r#"
            <export version="D">
              <components>
                <comp ref="U1">
                  <value>ATmega4808-AU</value>
                  <footprint>Package_QFP:TQFP-32</footprint>
                  <datasheet>https://example.test/mcu.pdf</datasheet>
                  <fields>
                    <field name="Manufacturer">Microchip</field>
                    <field name="MPN">ATmega4808-AU</field>
                  </fields>
                </comp>
                <comp ref="R1"><value>10k</value></comp>
              </components>
              <nets>
                <net code="1" name="LEFT_GO">
                  <node ref="U1" pin="3" pinfunction="PA3"/>
                  <node ref="R1" pin="1"/>
                </net>
              </nets>
            </export>
        "#;

        let graph = parse_kicad_xml_reader(Cursor::new(xml.as_bytes())).expect("valid XML");
        assert_eq!(graph.components.len(), 2);
        assert_eq!(graph.components[0].mpn.as_deref(), Some("ATmega4808-AU"));
        assert_eq!(graph.nets[0].name.as_deref(), Some("LEFT_GO"));
        assert_eq!(graph.nets[0].pins.len(), 2);
        assert_eq!(graph.pins[0].name.as_deref(), Some("PA3"));
    }
}
