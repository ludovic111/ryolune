//! A plugin's parameters and programs as an agent works with them: every parameter with its
//! display text, normalized position and automation state; parameters found by name; values
//! given as plain numbers, as 0-1 positions or as the text the plugin shows ("-6 dB",
//! "Hall"); factory programs listed and loaded. Values are document state
//! (`Insert.params`), and a program loaded as state lands in `Insert.blob`, so every change
//! here is one undo step, the same as a knob turned in the window.

use crate::{
    automation::AutomationTarget,
    control::{
        self, edit, opt, query, req, selected_plugin, Args, Host, Kind, Param, Spec, TRACK_ID,
    },
    control_refs::score,
    host as plugin_host,
    model::*,
    plugin::{Editor, ParamInfo},
    preset,
    store::Command,
    Result,
};
use serde_json::{json, Map, Value};

const SLOT: Param = opt(
    "slot",
    Kind::Integer,
    "Insert slot 0-7. Omit for the MIDI track's instrument.",
);
const PARAMETER: Param = opt(
    "parameter",
    Kind::String,
    "Parameter name, matched like a search (\"cutoff\", \"mix\"), or its id as text. Use instead of parameterId.",
);

pub const SPECS: &[Spec] = &[
    query("strip.parameters", "Read the parameters of the plugin on a strip (a track's instrument or insert, or a bus insert): id, name, plain value, display text as the plugin shows it, normalized 0-1 position, min, max, default, unit, steps, labels, whether it can be automated and the automation lane that drives it. Filter by name with query; pages of `limit`.", &[
        TRACK_ID, SLOT,
        opt("query", Kind::String, "Only parameters whose name matches these words, best match first (\"filter cutoff\")."),
        opt("changed", Kind::Boolean, "Only parameters set away from their default (default false)."),
        opt("offset", Kind::Integer, "Zero-based offset into the result, default 0."),
        opt("limit", Kind::Integer, "Parameters to return, 1-10000, default 200."),
    ]),
    edit("strip.setParameter", "Set one plugin parameter in one undo step. Name it by parameterId or parameter (its name); give exactly one of value (plain, within min-max), normalized (0-1 of the range) or text (what the plugin displays, such as \"-6 dB\", \"50%\" or a label like \"Hall\"). Answers with the parameter as it now reads.", &[
        TRACK_ID, SLOT,
        opt("parameterId", Kind::Integer, "Parameter id from strip.parameters."),
        PARAMETER,
        opt("value", Kind::Number, "Plain value within the parameter's min and max."),
        opt("normalized", Kind::Number, "Position 0-1 along the range, following its log or stepped scale."),
        opt("text", Kind::String, "Display text to parse: \"-6 dB\", \"440 Hz\", \"2.5k\", \"50%\" (of the range), \"On\", or a label."),
    ]),
    edit("strip.setParameters", "Set several plugin parameters atomically in one undo step. Keys are parameter ids or names; each value is a plain number, a display string (\"-6 dB\", \"Hall\") or {\"normalized\": 0-1}.", &[
        TRACK_ID, SLOT,
        req("values", Kind::Object, "Object mapping parameter ids or names to a plain number, a display string or {normalized}."),
    ]),
    query("strip.programs", "List the programs of the plugin on a strip: its own factory programs (Audio Unit factory presets, a VST3 program list) with the current one, and the ryolune presets saved for it (preset.list). CLAP preset discovery and plugins that only show presets in their own window are not listed.", &[TRACK_ID, SLOT]),
    edit("strip.setProgram", "Load one of the plugin's programs, by index or by name, in one undo step. A name that is not a factory program loads the ryolune preset of that name (preset.load).", &[
        TRACK_ID, SLOT,
        opt("index", Kind::Integer, "Program index from strip.programs."),
        opt("name", Kind::String, "Program or preset name."),
    ]),
    edit("strip.removeInsert", "Empty an insert slot on a track or bus, removing the plugin, its settings and any automation of it (one undo step).", &[
        TRACK_ID,
        req("slot", Kind::Integer, "Insert slot 0-7."),
    ]),
];

/// Run `f` on the plugin's editor: the instance the window already has loaded, else a fresh
/// one restored from the saved state. Parameter values in the document still win over what
/// the editor reports; callers read them from the insert.
pub fn with_editor<H: Host + ?Sized, R>(
    host: &mut H,
    insert: &Insert,
    f: impl FnOnce(&mut dyn Editor) -> Result<R>,
) -> Result<R> {
    if let Some(editor) = host.loaded_editor(insert) {
        return f(editor);
    }
    let mut instance = plugin_host::instantiate(&insert.plugin_id(), &insert.name, 48000)?;
    if !insert.blob.is_empty() {
        instance
            .editor
            .load(&plugin_host::decode_blob(&insert.blob)?)?;
    }
    f(instance.editor.as_mut())
}

fn round(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

/// One parameter as the registry reports it.
pub fn parameter_json(editor: &dyn Editor, p: &ParamInfo, value: f64, lane: Option<&str>) -> Value {
    let mut v = json!({
        "id": p.id, "name": p.name, "value": value,
        "display": editor.text(p.id, value),
        "normalized": round(p.normalize(value)),
        "min": p.min, "max": p.max, "default": p.default, "unit": p.unit,
        "steps": p.steps, "logarithmic": p.log, "labels": p.labels,
        "automatable": editor.automatable(p.id),
    });
    if let Some(lane) = lane {
        v["automationLane"] = json!(lane);
    }
    v
}

fn value_of(editor: &dyn Editor, insert: &Insert, p: &ParamInfo) -> f64 {
    insert
        .params
        .get(&p.id)
        .copied()
        .or_else(|| editor.value(p.id))
        .unwrap_or(p.default)
}

/// Automation lanes on this insert, by parameter id.
fn lanes(s: &Session, track: &str, insert: &Insert) -> Vec<(u32, String)> {
    s.automation
        .iter()
        .filter_map(|lane| match &lane.target {
            AutomationTarget::PluginParameter {
                track_id,
                insert_id,
                parameter_id,
                ..
            } if track_id == track && *insert_id == insert.id => {
                Some((*parameter_id, lane.id.clone()))
            }
            _ => None,
        })
        .collect()
}

/// Every parameter of a strip's plugin, the full list (`Host::plugin_parameters`).
pub fn all_parameters<H: Host + ?Sized>(
    host: &mut H,
    track: &str,
    slot: Option<usize>,
) -> Result<Value> {
    let insert = selected_plugin(host.store().session(), track, slot)?;
    let lanes = lanes(host.store().session(), track, &insert);
    with_editor(host, &insert, |editor| {
        Ok(json!({
            "pluginId": insert.plugin_id(),
            "parameters": editor.params().iter().map(|p| {
                let lane = lanes.iter().find(|(id, _)| *id == p.id).map(|(_, l)| l.as_str());
                parameter_json(editor, p, value_of(editor, &insert, p), lane)
            }).collect::<Vec<_>>(),
        }))
    })
}

/// Find a parameter by id-as-text or name: an exact name wins, then the single best match.
pub fn find_parameter<'a>(
    params: &'a [ParamInfo],
    wanted: &str,
    plugin: &str,
) -> Result<&'a ParamInfo> {
    let wanted = wanted.trim();
    if let Ok(id) = wanted.parse::<u32>() {
        if let Some(p) = params.iter().find(|p| p.id == id) {
            return Ok(p);
        }
    }
    let exact: Vec<&ParamInfo> = params
        .iter()
        .filter(|p| p.name.trim().eq_ignore_ascii_case(wanted))
        .collect();
    if let [one] = exact.as_slice() {
        return Ok(one);
    }
    let pool: Vec<&ParamInfo> = if exact.is_empty() {
        params.iter().collect()
    } else {
        exact
    };
    let mut scored: Vec<(u32, &ParamInfo)> = pool
        .into_iter()
        .filter_map(|p| score(wanted, &p.name).map(|s| (s, p)))
        .collect();
    scored.sort_by_key(|a| std::cmp::Reverse(a.0));
    let describe = |p: &ParamInfo| format!("{} (id {})", p.name, p.id);
    match scored.as_slice() {
        [] => {
            let near = crate::control_refs::suggest(wanted, params.iter().map(|p| p.name.as_str()))
                .map(|n| format!(" Did you mean {n}?"))
                .unwrap_or_default();
            Err(format!(
                "{plugin} has no parameter matching `{wanted}`.{near} Search with strip.parameters query=…"
            ))
        }
        [(best, p), rest @ ..] if rest.first().is_none_or(|(next, _)| next < best) => Ok(p),
        tied => Err(format!(
            "`{wanted}` matches several parameters of {plugin}: {}. Pass parameterId or a fuller name.",
            tied.iter()
                .take(10)
                .map(|(_, p)| describe(p))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// A plain value from one of the three ways of giving it.
fn plain_value(editor: &dyn Editor, p: &ParamInfo, given: &Value) -> Result<f64> {
    let checked = |v: f64| {
        if v.is_finite() && (p.min..=p.max).contains(&v) {
            Ok(v)
        } else {
            Err(format!(
                "{} must be between {} and {} (got {v}); or pass normalized 0-1 or text",
                p.name, p.min, p.max
            ))
        }
    };
    match given {
        Value::Number(n) => checked(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(text) => editor.parse_text(p.id, text).map_or_else(
            || {
                Err(format!(
                    "{} cannot read `{text}`.{} It shows values like \"{}\"",
                    p.name,
                    if p.labels.is_empty() {
                        String::new()
                    } else {
                        format!(" Choices: {}.", p.labels.join(", "))
                    },
                    editor.text(p.id, p.default)
                ))
            },
            checked,
        ),
        Value::Object(o) => {
            if let Some(n) = o.get("normalized").and_then(Value::as_f64) {
                if !(0.0..=1.0).contains(&n) {
                    return Err(format!("normalized for {} must be 0-1", p.name));
                }
                Ok(p.denormalize(n))
            } else if let Some(text) = o.get("text") {
                plain_value(editor, p, text)
            } else if let Some(v) = o.get("value") {
                plain_value(editor, p, v)
            } else {
                Err(format!(
                    "Give {} a number, a display string or {{\"normalized\": 0-1}}",
                    p.name
                ))
            }
        }
        _ => Err(format!(
            "Give {} a number, a display string or {{\"normalized\": 0-1}}",
            p.name
        )),
    }
}

/// Store the insert back into its strip in one undo step.
fn put_insert(host: &mut dyn Host, track: &str, slot: Option<usize>, insert: Insert) -> Result<()> {
    let mut strip = control::full_strip(host.store().session(), track);
    match slot {
        Some(slot) => strip.inserts[slot] = insert,
        None => strip.synth = Some(insert),
    }
    host.dispatch(Command::SetStrip {
        track: track.into(),
        strip,
    })
    .map(|_| ())
}

fn instrument_track(host: &dyn Host, track: &str, slot: Option<usize>) -> Result<()> {
    control::check_strip(host.store().session(), track)?;
    if slot.is_none() && is_bus(track) {
        return Err("Buses have no instrument; pass slot for one of their inserts".into());
    }
    if slot.is_none() && control::find_track(host.store().session(), track)?.kind != "midi" {
        return Err("Only MIDI tracks have an instrument; pass slot for an insert".into());
    }
    Ok(())
}

pub(crate) fn call(host: &mut dyn Host, name: &str, a: &Args) -> Result<Value> {
    let track = a.str("trackId")?;
    let slot = control::plugin_slot(a)?;
    match name {
        "strip.parameters" => {
            let insert = selected_plugin(host.store().session(), track, slot)?;
            let lanes = lanes(host.store().session(), track, &insert);
            let limit = a.opt_int("limit").unwrap_or(200);
            let offset = a.opt_int("offset").unwrap_or(0);
            if !(1..=10_000).contains(&limit) || offset < 0 {
                return Err("limit must be 1-10000 and offset non-negative".into());
            }
            let (query, changed) = (a.opt_str("query"), a.opt_bool("changed").unwrap_or(false));
            let format = crate::plugin::Format::parse(&insert.plugin_id())
                .map(|(f, _)| f.prefix())
                .unwrap_or("stock");
            with_editor(host, &insert, |editor| {
                let mut rows: Vec<(u32, &ParamInfo, f64)> = editor
                    .params()
                    .iter()
                    .filter_map(|p| {
                        let value = value_of(editor, &insert, p);
                        if changed && (value - p.default).abs() <= 1e-9 {
                            return None;
                        }
                        match query {
                            Some(q) => score(q, &p.name)
                                .or_else(|| (q.trim() == p.id.to_string()).then_some(2000))
                                .map(|s| (s, p, value)),
                            None => Some((0, p, value)),
                        }
                    })
                    .collect();
                if query.is_some() {
                    rows.sort_by_key(|a| std::cmp::Reverse(a.0));
                }
                let total = rows.len();
                let start = (offset as usize).min(total);
                let end = start.saturating_add(limit as usize).min(total);
                let parameters: Vec<Value> = rows[start..end]
                    .iter()
                    .map(|(_, p, value)| {
                        let lane = lanes
                            .iter()
                            .find(|(id, _)| *id == p.id)
                            .map(|(_, l)| l.as_str());
                        parameter_json(editor, p, *value, lane)
                    })
                    .collect();
                let mut out = json!({
                    "trackId": track, "slot": slot, "insertId": insert.id,
                    "pluginId": insert.plugin_id(), "plugin": insert.name, "format": format,
                    "bypassed": insert.state == "bypassed",
                    "latency": editor.latency(), "hasGui": editor.has_gui(),
                    "parameterCount": editor.params().len(),
                    "total": total, "offset": start, "parameters": parameters,
                    "nextOffset": if end < total { Some(end) } else { None },
                });
                if end < total && query.is_none() {
                    out["hint"] = json!("Large plugin: narrow with query=\"words\" or changed=true, or page with offset.");
                }
                Ok(out)
            })
        }
        "strip.setParameter" | "strip.setParameters" => {
            instrument_track(host, track, slot)?;
            let mut insert = selected_plugin(host.store().session(), track, slot)?;
            let snapshot = insert.clone();
            let label = snapshot.name.clone();
            let wanted: Vec<(Value, Value)> = if name == "strip.setParameter" {
                let reference = match (a.opt_int("parameterId"), a.opt_str("parameter")) {
                    (Some(id), None) => json!(id.to_string()),
                    (None, Some(name)) => json!(name),
                    _ => {
                        return Err(
                            "Name the parameter with parameterId or parameter (one of them)".into(),
                        )
                    }
                };
                let given: Vec<Value> = ["value", "normalized", "text"]
                    .iter()
                    .filter_map(|key| a.get(key).map(|v| (key, v)))
                    .map(|(key, v)| match *key {
                        "normalized" => json!({ "normalized": v }),
                        _ => v.clone(),
                    })
                    .collect();
                let [given] = given.as_slice() else {
                    return Err("Give exactly one of value, normalized or text".into());
                };
                vec![(reference, given.clone())]
            } else {
                let values = a
                    .get("values")
                    .and_then(Value::as_object)
                    .ok_or("Expected parameter values")?;
                if values.is_empty() || values.len() > 512 {
                    return Err("Set between 1 and 512 parameters per call".into());
                }
                values.iter().map(|(k, v)| (json!(k), v.clone())).collect()
            };
            let changes = with_editor(host, &snapshot, |editor| {
                let params = editor.params().to_vec();
                wanted
                    .iter()
                    .map(|(reference, given)| {
                        let reference = reference.as_str().unwrap_or_default();
                        let p = find_parameter(&params, reference, &label)?;
                        let value = plain_value(editor, p, given)?;
                        Ok((p.id, value, parameter_json(editor, p, value, None)))
                    })
                    .collect::<Result<Vec<_>>>()
            })?;
            for (id, value, _) in &changes {
                insert.params.insert(*id, *value);
            }
            put_insert(host, track, slot, insert)?;
            let mut out = control::strip_json(host.store().session(), track);
            out["changed"] = Value::Array(changes.into_iter().map(|c| c.2).collect());
            Ok(out)
        }
        "strip.programs" => {
            let insert = selected_plugin(host.store().session(), track, slot)?;
            let plugin_id = insert.plugin_id();
            let (programs, parameter, current) = with_editor(host, &insert, |editor| {
                let programs = editor.programs();
                let parameter = editor.program_parameter();
                let current = match parameter {
                    Some(id) => {
                        let value = insert.params.get(&id).copied().or_else(|| editor.value(id));
                        let steps = programs.len().saturating_sub(1).max(1) as f64;
                        value.map(|v| (v * steps).round() as usize)
                    }
                    // An Audio Unit reports its present preset, restored with its state.
                    None => editor.current_program(),
                };
                Ok((programs, parameter, current))
            })?;
            let presets: Vec<Value> = preset::list(Some(&plugin_id))?
                .iter()
                .map(|p| json!({ "name": p.name, "factory": p.factory }))
                .collect();
            Ok(json!({
                "pluginId": plugin_id, "plugin": insert.name,
                "programs": programs.iter().enumerate().map(|(i, n)| json!({"index": i, "name": n})).collect::<Vec<_>>(),
                "current": current,
                "source": if programs.is_empty() { "none" } else if parameter.is_some() { "programParameter" } else { "factoryPresets" },
                "presets": presets,
            }))
        }
        "strip.setProgram" => {
            instrument_track(host, track, slot)?;
            let mut insert = selected_plugin(host.store().session(), track, slot)?;
            let snapshot = insert.clone();
            let (programs, parameter) = with_editor(host, &snapshot, |editor| {
                Ok((editor.programs(), editor.program_parameter()))
            })?;
            let index = match (a.opt_int("index"), a.opt_str("name")) {
                (Some(i), None) => {
                    if i < 0 || i as usize >= programs.len() {
                        return Err(format!(
                            "{} has {} programs; index must be 0-{}",
                            insert.name,
                            programs.len(),
                            programs.len().saturating_sub(1)
                        ));
                    }
                    i as usize
                }
                (None, Some(wanted)) => {
                    let exact = programs
                        .iter()
                        .position(|p| p.trim().eq_ignore_ascii_case(wanted.trim()));
                    let fuzzy = || {
                        let mut best: Vec<(u32, usize)> = programs
                            .iter()
                            .enumerate()
                            .filter_map(|(i, p)| score(wanted, p).map(|s| (s, i)))
                            .collect();
                        best.sort_by_key(|a| std::cmp::Reverse(a.0));
                        match best.as_slice() {
                            [(s, i), rest @ ..] if rest.first().is_none_or(|(n, _)| n < s) => {
                                Some(*i)
                            }
                            _ => None,
                        }
                    };
                    match exact.or_else(fuzzy) {
                        Some(i) => i,
                        None => {
                            // Not a factory program: a ryolune preset of that name.
                            let mut params = json!({ "trackId": track, "name": wanted });
                            if let Some(slot) = slot {
                                params["slot"] = json!(slot);
                            }
                            return control::call(host, "preset.load", &params, false).map_err(
                                |e| {
                                    format!(
                                        "{e}. Programs of {}: {}",
                                        insert.name,
                                        if programs.is_empty() {
                                            "none".into()
                                        } else {
                                            programs
                                                .iter()
                                                .take(30)
                                                .cloned()
                                                .collect::<Vec<_>>()
                                                .join(", ")
                                        }
                                    )
                                },
                            );
                        }
                    }
                }
                _ => return Err("Give index or name (one of them)".into()),
            };
            if let Some(id) = parameter {
                let steps = programs.len().saturating_sub(1).max(1) as f64;
                insert.params.insert(id, index as f64 / steps);
            } else {
                // Loaded into a fresh instance, never the one playing: the saved state is the
                // document's, and the window restores it on the audio thread like any other.
                let mut instance =
                    plugin_host::instantiate(&insert.plugin_id(), &insert.name, 48000)?;
                if !insert.blob.is_empty() {
                    instance
                        .editor
                        .load(&plugin_host::decode_blob(&insert.blob)?)?;
                }
                instance.editor.load_program(index)?;
                let state = instance
                    .editor
                    .save()
                    .ok_or("The plugin did not return its state after loading the program")?;
                insert.blob = plugin_host::encode_blob(&state);
                insert.params.clear();
            }
            put_insert(host, track, slot, insert)?;
            let mut out = control::strip_json(host.store().session(), track);
            out["program"] = json!({ "index": index, "name": programs[index] });
            Ok(out)
        }
        "strip.removeInsert" => {
            control::check_strip(host.store().session(), track)?;
            let slot = slot.ok_or("strip.removeInsert needs `slot`")?;
            let mut strip = control::full_strip(host.store().session(), track);
            if strip.inserts[slot].is_empty() {
                return Err(format!("Insert slot {slot} is already empty"));
            }
            let removed = strip.inserts[slot].name.clone();
            strip.inserts[slot] = Insert::empty_slot();
            host.dispatch(Command::SetStrip {
                track: track.into(),
                strip,
            })?;
            let mut out = control::strip_json(host.store().session(), track);
            out["removed"] = json!(removed);
            Ok(out)
        }
        _ => Err(format!(
            "Command `{name}` is registered but not implemented"
        )),
    }
}

/// The few parameters worth showing in an overview: those set away from their default, as
/// the plugin displays them. Only for editors that are cheap to reach (stock plugins, or any
/// plugin the window has loaded).
pub fn changed_summary(host: &mut dyn Host, insert: &Insert, max: usize) -> Option<Value> {
    if insert.params.is_empty() {
        return None;
    }
    let stock = insert.plugin_id().starts_with("stock:");
    if !stock && host.loaded_editor(insert).is_none() {
        let mut map = Map::new();
        for (id, v) in insert.params.iter().take(max) {
            map.insert(format!("#{id}"), json!(round(*v)));
        }
        return Some(Value::Object(map));
    }
    with_editor(host, insert, |editor| {
        let mut map = Map::new();
        for p in editor.params() {
            let Some(v) = insert.params.get(&p.id) else {
                continue;
            };
            if (v - p.default).abs() <= 1e-9 {
                continue;
            }
            if map.len() == max {
                map.insert(
                    "…".into(),
                    json!(format!("{} more", insert.params.len() - max)),
                );
                break;
            }
            map.insert(p.name.clone(), json!(editor.text(p.id, *v)));
        }
        Ok(Value::Object(map))
    })
    .ok()
    .filter(|v| v.as_object().is_some_and(|m| !m.is_empty()))
}
