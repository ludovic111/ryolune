//! LV2 State 2.8 (`state:interface`) and the blob ryolune keeps for an LV2 insert: the input
//! control ports by symbol plus every property the plugin stored, with keys, types and URID
//! values written as URIs so the blob means the same thing in the next process.

use super::features::{map, unmap};
use super::ffi::{self, uri, LV2_Feature, LV2_Handle, LV2_State_Interface};
use super::world::{Property, StateProperties};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, ffi::c_void};

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Blob {
    /// The blob format version.
    pub lv2: u32,
    pub plugin: String,
    #[serde(default)]
    pub ports: BTreeMap<String, f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state: Vec<BlobProperty>,
}
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct BlobProperty {
    pub key: String,
    #[serde(rename = "type")]
    pub type_uri: String,
    #[serde(default)]
    pub flags: u32,
    /// Base64 of the value; for `atom:URID` values, the URI text.
    pub value: String,
}
impl Blob {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let blob: Blob = serde_json::from_slice(bytes)
            .map_err(|e| format!("This is not the state of an LV2 plugin ({e})"))?;
        if blob.lv2 != 1 {
            return Err(format!(
                "This LV2 state was saved by a newer version (format {})",
                blob.lv2
            ));
        }
        Ok(blob)
    }
    pub fn properties(&self) -> Result<StateProperties, String> {
        let mut out = vec![];
        for p in &self.state {
            let value = if p.type_uri == urid_type() {
                let mut v = p.value.clone().into_bytes();
                v.push(0);
                v
            } else {
                STANDARD
                    .decode(&p.value)
                    .map_err(|e| format!("Damaged LV2 state value for {}: {e}", p.key))?
            };
            out.push(Property {
                key: p.key.clone(),
                type_uri: p.type_uri.clone(),
                value,
            });
        }
        Ok(StateProperties(out))
    }
}
fn urid_type() -> String {
    format!("{}URID", uri::ATOM)
}

unsafe extern "C" fn store(
    handle: *mut c_void,
    key: u32,
    value: *const c_void,
    size: usize,
    type_: u32,
    flags: u32,
) -> u32 {
    let out = &mut *(handle as *mut Vec<BlobProperty>);
    let (Some(key), Some(type_uri)) = (unmap(key), unmap(type_)) else {
        return ffi::LV2_STATE_ERR_UNKNOWN;
    };
    if size > 256 * 1024 * 1024 || (value.is_null() && size > 0) {
        return ffi::LV2_STATE_ERR_UNKNOWN;
    }
    let bytes: &[u8] = if size == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(value as *const u8, size)
    };
    let value = if type_uri == urid_type() && size == 4 {
        let id = u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        unmap(id).unwrap_or_default()
    } else {
        STANDARD.encode(bytes)
    };
    out.retain(|p| p.key != key);
    out.push(BlobProperty {
        key,
        type_uri,
        flags,
        value,
    });
    ffi::LV2_STATE_SUCCESS
}

/// Ask the plugin for its state. `None` when it has no state interface.
///
/// # Safety
/// `handle` is a live instance of the plugin that returned `interface`.
pub unsafe fn save(
    handle: LV2_Handle,
    interface: *const LV2_State_Interface,
    features: *const *const LV2_Feature,
) -> Result<Vec<BlobProperty>, String> {
    let Some(save) = interface.as_ref().and_then(|i| i.save) else {
        return Ok(vec![]);
    };
    let mut out: Vec<BlobProperty> = vec![];
    let status = save(
        handle,
        store,
        &mut out as *mut Vec<BlobProperty> as *mut c_void,
        ffi::LV2_STATE_IS_POD | ffi::LV2_STATE_IS_PORTABLE,
        features,
    );
    if status != ffi::LV2_STATE_SUCCESS {
        return Err(format!("The plugin could not save its state (status {status})"));
    }
    Ok(out)
}

struct Restoring {
    /// key, type, flags, value; values stay put until `restore` returns.
    properties: Vec<(u32, u32, u32, Vec<u8>)>,
}
unsafe extern "C" fn retrieve(
    handle: *mut c_void,
    key: u32,
    size: *mut usize,
    type_: *mut u32,
    flags: *mut u32,
) -> *const c_void {
    let r = &*(handle as *const Restoring);
    match r.properties.iter().find(|p| p.0 == key) {
        Some((_, t, f, value)) => {
            if !size.is_null() {
                *size = value.len();
            }
            if !type_.is_null() {
                *type_ = *t;
            }
            if !flags.is_null() {
                *flags = *f;
            }
            value.as_ptr() as *const c_void
        }
        None => std::ptr::null(),
    }
}

/// Give the plugin a state. URID values (written as URI text) are mapped first.
///
/// # Safety
/// `handle` is a live instance of the plugin that returned `interface`, and nothing else
/// calls into it meanwhile (LV2 puts `restore` in the instantiation class).
pub unsafe fn restore(
    handle: LV2_Handle,
    interface: *const LV2_State_Interface,
    features: *const *const LV2_Feature,
    state: &StateProperties,
) -> Result<(), String> {
    let Some(restore) = interface.as_ref().and_then(|i| i.restore) else {
        return if state.0.is_empty() {
            Ok(())
        } else {
            Err("The plugin has saved state but no way to restore it".into())
        };
    };
    let urid = urid_type();
    let properties = state
        .0
        .iter()
        .map(|p| {
            let value = if p.type_uri == urid {
                let text = String::from_utf8_lossy(p.value.strip_suffix(&[0]).unwrap_or(&p.value)).into_owned();
                map(&text).to_ne_bytes().to_vec()
            } else {
                p.value.clone()
            };
            (map(&p.key), map(&p.type_uri), ffi::LV2_STATE_IS_POD | ffi::LV2_STATE_IS_PORTABLE, value)
        })
        .collect();
    let restoring = Restoring { properties };
    let status = restore(
        handle,
        retrieve,
        &restoring as *const Restoring as *mut c_void,
        0,
        features,
    );
    if status != ffi::LV2_STATE_SUCCESS {
        return Err(format!("The plugin refused the state (status {status})"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stored_properties_come_back_with_their_uris() {
        let mut out: Vec<BlobProperty> = vec![];
        let key = map("http://example.org/key");
        let float = map(&format!("{}Float", uri::ATOM));
        let urid = map(&urid_type());
        let target = map("http://example.org/target");
        unsafe {
            let value = 0.25f32;
            store(&mut out as *mut _ as *mut c_void, key, &value as *const f32 as *const c_void, 4, float, 1);
            let other = map("http://example.org/other");
            store(&mut out as *mut _ as *mut c_void, other, &target as *const u32 as *const c_void, 4, urid, 1);
        }
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].value, "http://example.org/target");
        let blob = Blob {
            lv2: 1,
            plugin: "x".into(),
            ports: BTreeMap::new(),
            state: out,
        };
        let json = serde_json::to_vec(&blob).unwrap();
        let back = Blob::parse(&json).unwrap();
        let properties = back.properties().unwrap();
        assert_eq!(properties.0[0].value, 0.25f32.to_ne_bytes());
        assert_eq!(properties.0[1].value, b"http://example.org/target\0");
        assert!(Blob::parse(b"{\"lv2\": 9, \"plugin\": \"x\"}").unwrap_err().contains("newer"));
    }
}
