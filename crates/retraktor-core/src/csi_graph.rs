//! `CSI 1.0` Controller Script Interface reactive `Wire` / `WiresGroup` graph engine
//! and QML controller script parser/inspector.

use crate::property_bus::{PropValue, PropertyBus};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Combinatorial logic gate over multiple hardware surface inputs (`Or { inputs: [...] }`, `And { inputs: [...] }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogicGateKind {
    Or(Vec<String>),
    And(Vec<String>),
}

impl LogicGateKind {
    pub fn matches_control(&self, control: &str) -> bool {
        match self {
            LogicGateKind::Or(inputs) | LogicGateKind::And(inputs) => {
                inputs.iter().any(|i| i == control)
            }
        }
    }
}

/// Property adapter semantics matching `CSI 1.0` QML adapter types.
#[derive(Debug, Clone, PartialEq)]
pub enum AdapterKind {
    /// `DirectPropertyAdapter { path; input }` or `ValuePropertyAdapter { path }`
    Direct { path: String },
    /// `TriggerPropertyAdapter { path; output }`
    Trigger { path: String, output: bool },
    /// `TogglePropertyAdapter { path }`
    Toggle { path: String },
    /// `SetPropertyAdapter { path; value }`
    Set { path: String, value: PropValue },
    /// `HoldPropertyAdapter { path; value; defaultValue }`
    Hold {
        path: String,
        value: PropValue,
        default_value: PropValue,
    },
    /// `RelativePropertyAdapter { path; step; wrap }`
    Relative {
        path: String,
        step: f64,
        wrap: bool,
    },
    /// Symbolic endpoint (e.g. `"hotcues.1.trigger"`, `"browser.scrolling"`)
    Symbolic { target: String },
}

/// Endpoint of a `Wire`.
#[derive(Debug, Clone, PartialEq)]
pub struct WireEndpoint {
    pub surface_path: String,
    pub adapter: AdapterKind,
}

/// A single `CSI 1.0` `Wire { from: ...; to: ...; enabled: ... }`.
#[derive(Debug, Clone, PartialEq)]
pub struct CsiWire {
    pub from: String,
    pub gate: Option<LogicGateKind>,
    pub to: AdapterKind,
    pub enabled: bool,
}

impl CsiWire {
    pub fn new(from: impl Into<String>, to: AdapterKind, enabled: bool) -> Self {
        Self {
            from: from.into(),
            gate: None,
            to,
            enabled,
        }
    }
}

/// A hierarchical `WiresGroup { enabled: ...; Wire { ... } WiresGroup { ... } }`.
#[derive(Debug, Clone, Default)]
pub struct CsiWiresGroup {
    pub name: String,
    pub enabled: bool,
    pub wires: Vec<CsiWire>,
    pub children: Vec<CsiWiresGroup>,
}

/// Hardware surface control event delivered from NI HID or MIDI controller.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceEvent {
    Button { control: String, pressed: bool },
    Fader { control: String, value: f64 },
    Encoder { control: String, delta: i64 },
}

impl CsiWiresGroup {
    pub fn new(name: impl Into<String>, enabled: bool) -> Self {
        Self {
            name: name.into(),
            enabled,
            wires: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn add_wire(&mut self, wire: CsiWire) {
        self.wires.push(wire);
    }

    pub fn add_group(&mut self, group: CsiWiresGroup) {
        self.children.push(group);
    }

    /// Returns all currently active wires (where every ancestor `WiresGroup.enabled`
    /// and the `Wire.enabled` flag itself are `true`).
    pub fn active_wires(&self) -> Vec<&CsiWire> {
        let mut out = Vec::new();
        self.collect_active_wires(&mut out);
        out
    }

    fn collect_active_wires<'a>(&'a self, out: &mut Vec<&'a CsiWire>) {
        if !self.enabled {
            return;
        }
        for w in &self.wires {
            if w.enabled {
                out.push(w);
            }
        }
        for child in &self.children {
            child.collect_active_wires(out);
        }
    }

    /// Dispatches a hardware [`SurfaceEvent`] through all active wires in the graph,
    /// mutating the target [`PropertyBus`] and returning the number of property writes triggered.
    pub fn dispatch_event(&self, event: &SurfaceEvent, bus: &mut PropertyBus) -> usize {
        let active = self.active_wires();
        let mut writes = 0;

        let control_name = match event {
            SurfaceEvent::Button { control, .. }
            | SurfaceEvent::Fader { control, .. }
            | SurfaceEvent::Encoder { control, .. } => control.as_str(),
        };

        for wire in active {
            let matched = if let Some(gate) = &wire.gate {
                gate.matches_control(control_name)
            } else {
                wire.from == control_name
            };
            if !matched {
                continue;
            }
            match (&wire.to, event) {
                (AdapterKind::Direct { path }, SurfaceEvent::Fader { value, .. }) => {
                    if bus.set(path, PropValue::Float(*value)) {
                        writes += 1;
                    }
                }
                (AdapterKind::Direct { path }, SurfaceEvent::Button { pressed, .. }) => {
                    if bus.set(path, PropValue::Bool(*pressed)) {
                        writes += 1;
                    }
                }
                (AdapterKind::Trigger { path, .. }, SurfaceEvent::Button { pressed: true, .. }) => {
                    if bus.set(path, PropValue::Bool(true)) {
                        writes += 1;
                    }
                }
                (AdapterKind::Toggle { path }, SurfaceEvent::Button { pressed: true, .. }) => {
                    let cur = bus.get(path).and_then(|v| v.as_bool()).unwrap_or(false);
                    if bus.set(path, PropValue::Bool(!cur)) {
                        writes += 1;
                    }
                }
                (AdapterKind::Set { path, value }, SurfaceEvent::Button { pressed: true, .. }) => {
                    if bus.set(path, value.clone()) {
                        writes += 1;
                    }
                }
                (
                    AdapterKind::Hold {
                        path,
                        value,
                        default_value,
                    },
                    SurfaceEvent::Button { pressed, .. },
                ) => {
                    let target = if *pressed { value } else { default_value };
                    if bus.set(path, target.clone()) {
                        writes += 1;
                    }
                }
                (
                    AdapterKind::Relative { path, step, wrap },
                    SurfaceEvent::Encoder { delta, .. },
                ) => {
                    if let Some(meta) = bus.get_meta(path).cloned() {
                        match meta.value {
                            PropValue::Float(cur) => {
                                let mut next = cur + (*delta as f64) * (*step);
                                if *wrap {
                                    if let (Some(lo), Some(hi)) = (meta.min, meta.max) {
                                        let span = hi - lo;
                                        if span > 0.0 {
                                            next = lo + (next - lo).rem_euclid(span);
                                        }
                                    }
                                }
                                if bus.set(path, PropValue::Float(next)) {
                                    writes += 1;
                                }
                            }
                            PropValue::Int(cur) => {
                                let step_i = (*step).round().max(1.0) as i64;
                                let mut next = cur + (*delta) * step_i;
                                if *wrap {
                                    if let (Some(lo), Some(hi)) = (meta.min, meta.max) {
                                        let span = (hi as i64) - (lo as i64) + 1;
                                        if span > 0 {
                                            next = (lo as i64) + (next - (lo as i64)).rem_euclid(span);
                                        }
                                    }
                                }
                                if bus.set(path, PropValue::Int(next)) {
                                    writes += 1;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        writes
    }
}

/// Summary extracted by parsing a shipped Traktor Pro 4 `CSI/*.qml` file.
#[derive(Debug, Clone, Default)]
pub struct CsiModuleSummary {
    pub imports: Vec<String>,
    pub app_property_paths: Vec<String>,
    pub mapping_property_paths: Vec<String>,
    pub wire_count: usize,
    pub wires_group_count: usize,
    pub wire_endpoints: Vec<(String, String)>,
}

impl CsiModuleSummary {
    /// Parses a Traktor Pro 4 QML controller script (`CSI/*.qml`) and extracts all `import`
    /// statements, `AppProperty` bindings, `MappingPropertyDescriptor` definitions, and
    /// `Wire` / `WiresGroup` connections.
    pub fn parse_qml(qml_source: &str) -> Self {
        let mut summary = Self::default();
        let mut in_wire = false;
        let mut current_from: Option<String> = None;
        let mut current_to: Option<String> = None;

        for raw_line in qml_source.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with("//") {
                continue;
            }

            if line.starts_with("import ") {
                summary.imports.push(line.to_string());
            }

            if line == "WiresGroup"
                || (line.starts_with("WiresGroup")
                    && (line.contains('{') || line.ends_with("WiresGroup")))
            {
                summary.wires_group_count += 1;
            }

            if line == "Wire"
                || ((line.starts_with("Wire ") || line.starts_with("Wire{"))
                    && (line.contains('{') || line.ends_with("Wire")))
            {
                summary.wire_count += 1;
                in_wire = true;
                current_from = None;
                current_to = None;
            }

            if line.contains("AppProperty") {
                if let Some(p) = extract_quoted_after(line, "path:") {
                    summary.app_property_paths.push(p);
                }
            } else if line.starts_with("path:") && !in_wire {
                if let Some(p) = extract_quoted_after(line, "path:") {
                    if p.starts_with("app.traktor.") {
                        summary.app_property_paths.push(p);
                    } else {
                        summary.mapping_property_paths.push(p);
                    }
                }
            }

            if line.contains("MappingPropertyDescriptor") {
                if let Some(p) = extract_quoted_after(line, "path:") {
                    summary.mapping_property_paths.push(p);
                }
            }

            if in_wire {
                if let Some(f) = extract_quoted_after(line, "from:") {
                    current_from = Some(f);
                }
                if let Some(t) = extract_quoted_after(line, "to:") {
                    current_to = Some(t);
                }
                if line.contains("PropertyAdapter") {
                    if let Some(p) = extract_quoted_after(line, "path:") {
                        if p.starts_with("app.traktor.") {
                            summary.app_property_paths.push(p.clone());
                        }
                        current_to = Some(p);
                    }
                }
                if line.contains('}') {
                    if let (Some(f), Some(t)) = (current_from.take(), current_to.take()) {
                        summary.wire_endpoints.push((f, t));
                    }
                    in_wire = false;
                }
            }
        }

        summary
    }
}

/// Aggregate report across multiple QML files in a controller directory.
#[derive(Debug, Clone, Default)]
pub struct CsiDirectoryReport {
    pub files_scanned: usize,
    pub total_wires: usize,
    pub total_wires_groups: usize,
    pub unique_app_properties: BTreeSet<String>,
    pub unique_mapping_properties: BTreeSet<String>,
}

impl CsiDirectoryReport {
    pub fn scan_path(path: &Path) -> Result<Self, String> {
        let mut report = Self::default();
        Self::visit_path(path, &mut report)?;
        Ok(report)
    }

    fn visit_path(path: &Path, report: &mut Self) -> Result<(), String> {
        if path.is_file() {
            if path.extension().and_then(|e| e.to_str()) == Some("qml") {
                let content = fs::read_to_string(path)
                    .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
                let s = CsiModuleSummary::parse_qml(&content);
                report.files_scanned += 1;
                report.total_wires += s.wire_count;
                report.total_wires_groups += s.wires_group_count;
                for p in s.app_property_paths {
                    // Skip incomplete dynamic string concatenations ending in '.'
                    if !p.ends_with('.') {
                        report.unique_app_properties.insert(p);
                    }
                }
                for m in s.mapping_property_paths {
                    if !m.ends_with('.') {
                        report.unique_mapping_properties.insert(m);
                    }
                }
            }
            return Ok(());
        }

        if path.is_dir() {
            let entries = fs::read_dir(path)
                .map_err(|e| format!("Failed to read dir {}: {e}", path.display()))?;
            for entry in entries.flatten() {
                let _ = Self::visit_path(&entry.path(), report);
            }
        }
        Ok(())
    }
}

fn extract_quoted_after(line: &str, needle: &str) -> Option<String> {
    let idx = line.find(needle)?;
    let rest = &line[idx + needle.len()..];
    let first_quote = rest.find('"')?;
    let after_first = &rest[first_quote + 1..];
    let second_quote = after_first.find('"')?;
    Some(after_first[..second_quote].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wires_group_gating_and_adapters() {
        let mut bus = PropertyBus::new_traktor_pro4_default();
        let mut root = CsiWiresGroup::new("S8_Left_Pads", true);

        let mut hotcues_group = CsiWiresGroup::new("HotcuesMode", true);
        hotcues_group.add_wire(CsiWire::new(
            "s8.left.pads.1",
            AdapterKind::Toggle {
                path: "app.traktor.decks.1.play".to_string(),
            },
            true,
        ));

        let mut freeze_group = CsiWiresGroup::new("FreezeMode", false);
        freeze_group.add_wire(CsiWire::new(
            "s8.left.pads.1",
            AdapterKind::Set {
                path: "app.traktor.decks.1.freeze.enabled".to_string(),
                value: PropValue::Bool(true),
            },
            true,
        ));

        root.add_group(hotcues_group);
        root.add_group(freeze_group);

        assert_eq!(root.active_wires().len(), 1);

        // Press pad 1 -> should toggle deck 1 play to true, but NOT enable freeze
        let evt = SurfaceEvent::Button {
            control: "s8.left.pads.1".to_string(),
            pressed: true,
        };
        let writes = root.dispatch_event(&evt, &mut bus);
        assert_eq!(writes, 1);
        assert_eq!(
            bus.get("app.traktor.decks.1.play"),
            Some(&PropValue::Bool(true))
        );
        assert_eq!(
            bus.get("app.traktor.decks.1.freeze.enabled"),
            Some(&PropValue::Bool(false))
        );
    }

    #[test]
    fn test_parse_shipped_style_qml() {
        let sample_qml = r#"
import CSI 1.0
import Defines 1.0

Module {
    AppProperty { id: propPlay; path: "app.traktor.decks.1.play" }
    MappingPropertyDescriptor { id: padsMode; path: "mapping.state.left.pads_mode"; type: MappingPropertyDescriptor.Integer; value: PadsMode.hotcues }
    WiresGroup {
        enabled: true
        Wire { from: "%surface%.play"; to: TogglePropertyAdapter { path: "app.traktor.decks.1.play" } }
        Wire { from: "%surface%.sync"; to: "transport.sync" }
    }
}
"#;
        let summary = CsiModuleSummary::parse_qml(sample_qml);
        assert_eq!(summary.imports.len(), 2);
        assert_eq!(summary.app_property_paths.len(), 2);
        assert_eq!(
            summary.mapping_property_paths,
            vec!["mapping.state.left.pads_mode"]
        );
        assert_eq!(summary.wires_group_count, 1);
        assert_eq!(summary.wire_count, 2);
        assert_eq!(summary.wire_endpoints.len(), 2);
    }
}
