//! Continuum GC's core options (libretro SET_CORE_OPTIONS_V2, with the plain SET_VARIABLES list
//! for older frontends), keyed `cgc_` (.kiro/notes/docs/CORE_OPTIONS_STANDARD.md).

use std::ffi::{c_char, c_void, CStr};

type EnvFn = unsafe extern "C" fn(u32, *mut c_void) -> bool;

const ENV_GET_VARIABLE: u32 = 15;
const ENV_SET_VARIABLES: u32 = 16;
const ENV_GET_VARIABLE_UPDATE: u32 = 17;
const ENV_SET_CORE_OPTIONS_V2: u32 = 67;

#[repr(C)]
#[derive(Clone, Copy)]
struct OptValue {
    value: *const c_char,
    label: *const c_char,
}

#[repr(C)]
struct OptCategory {
    key: *const c_char,
    desc: *const c_char,
    info: *const c_char,
}

#[repr(C)]
struct OptDefinition {
    key: *const c_char,
    desc: *const c_char,
    desc_categorized: *const c_char,
    info: *const c_char,
    info_categorized: *const c_char,
    category_key: *const c_char,
    values: [OptValue; 128],
    default_value: *const c_char,
}

#[repr(C)]
struct OptionsV2 {
    categories: *const OptCategory,
    definitions: *const OptDefinition,
}

#[repr(C)]
struct Variable {
    key: *const c_char,
    value: *const c_char,
}

const NULL_VALUE: OptValue = OptValue { value: std::ptr::null(), label: std::ptr::null() };

/// The speed read-out (performance): how fast the game runs and what takes the time, drawn in
/// the corner of the picture.
pub const SPEED_READOUT: &CStr = c"cgc_speed_readout";

/// (category, key, label, info, values, default)
type Spec = (&'static CStr, &'static CStr, &'static CStr, &'static CStr, &'static [(&'static CStr, &'static CStr)], &'static CStr);

const SPECS: &[Spec] = &[(
    c"performance",
    SPEED_READOUT,
    c"Speed read-out",
    c"Shows how fast the game runs, and what takes the time, in the corner. Applies right away.",
    &[(c"off", c"Off"), (c"on", c"On")],
    c"off",
)];

const CATEGORIES: &[(&CStr, &CStr, &CStr)] = &[(c"performance", c"Performance", c"How fast the game runs.")];

/// Declares the options to the frontend. Called from retro_set_environment.
///
/// # Safety
/// `env` must be the frontend's environment callback.
pub unsafe fn declare(env: EnvFn) {
    // Built once and leaked: the frontend may keep the pointers.
    let defs: &'static mut Vec<OptDefinition> = Box::leak(Box::new(Vec::new()));
    for (cat, key, desc, info, values, default) in SPECS {
        let mut vals = [NULL_VALUE; 128];
        for (i, (v, l)) in values.iter().enumerate() {
            vals[i] = OptValue { value: v.as_ptr(), label: l.as_ptr() };
        }
        defs.push(OptDefinition {
            key: key.as_ptr(),
            desc: desc.as_ptr(),
            desc_categorized: desc.as_ptr(),
            info: info.as_ptr(),
            info_categorized: info.as_ptr(),
            category_key: cat.as_ptr(),
            values: vals,
            default_value: default.as_ptr(),
        });
    }
    defs.push(OptDefinition {
        key: std::ptr::null(),
        desc: std::ptr::null(),
        desc_categorized: std::ptr::null(),
        info: std::ptr::null(),
        info_categorized: std::ptr::null(),
        category_key: std::ptr::null(),
        values: [NULL_VALUE; 128],
        default_value: std::ptr::null(),
    });
    let cats: &'static mut Vec<OptCategory> = Box::leak(Box::new(Vec::new()));
    for (k, d, i) in CATEGORIES {
        cats.push(OptCategory { key: k.as_ptr(), desc: d.as_ptr(), info: i.as_ptr() });
    }
    cats.push(OptCategory { key: std::ptr::null(), desc: std::ptr::null(), info: std::ptr::null() });
    let mut opts = OptionsV2 { categories: cats.as_ptr(), definitions: defs.as_ptr() };
    if env(ENV_SET_CORE_OPTIONS_V2, &mut opts as *mut OptionsV2 as *mut c_void) {
        return;
    }
    // Older frontends: "Label; default|other|...".
    let texts: &'static mut Vec<std::ffi::CString> = Box::leak(Box::new(Vec::new()));
    let vars: &'static mut Vec<Variable> = Box::leak(Box::new(Vec::new()));
    for (_, key, desc, _, values, default) in SPECS {
        let mut list = vec![default.to_string_lossy().into_owned()];
        for (v, _) in values.iter() {
            if v != default {
                list.push(v.to_string_lossy().into_owned());
            }
        }
        texts.push(std::ffi::CString::new(format!("{}; {}", desc.to_string_lossy(), list.join("|"))).unwrap_or_default());
        vars.push(Variable { key: key.as_ptr(), value: texts.last().map_or(std::ptr::null(), |t| t.as_ptr()) });
    }
    vars.push(Variable { key: std::ptr::null(), value: std::ptr::null() });
    env(ENV_SET_VARIABLES, vars.as_mut_ptr() as *mut c_void);
}

/// An option's value, from the frontend.
///
/// # Safety
/// `env` must be the frontend's environment callback.
pub unsafe fn get(env: EnvFn, key: &CStr) -> Option<String> {
    let mut var = Variable { key: key.as_ptr(), value: std::ptr::null() };
    if env(ENV_GET_VARIABLE, &mut var as *mut Variable as *mut c_void) && !var.value.is_null() {
        Some(CStr::from_ptr(var.value).to_string_lossy().into_owned())
    } else {
        None
    }
}

/// Whether the player changed an option since the last look.
///
/// # Safety
/// `env` must be the frontend's environment callback.
pub unsafe fn changed(env: EnvFn) -> bool {
    let mut updated = false;
    env(ENV_GET_VARIABLE_UPDATE, &mut updated as *mut bool as *mut c_void) && updated
}

/// Whether the speed read-out is on.
///
/// # Safety
/// `env` must be the frontend's environment callback.
pub unsafe fn speed_readout(env: EnvFn) -> bool {
    get(env, SPEED_READOUT).is_some_and(|v| v == "on")
}
