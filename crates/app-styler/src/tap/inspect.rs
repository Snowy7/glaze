//! `mypaper-styler --dump PID --props SELECTOR`: the property values of the elements a selector
//! picks, with where each comes from (local, style, template, visual state...), so rules can
//! target the property and element that actually draw.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::sync::Arc;

use super::{At, Session, log, post};
use crate::selector::Selector;
use crate::xamlom::{
    IS_VALUE_HANDLE, InstanceHandle, PropertyChainSource, PropertyChainValue, free_chain, text,
};

/// `BaseValueSource` names, by value (xamlOM.h).
const SOURCES: [&str; 15] = [
    "unknown",
    "default",
    "built-in style",
    "style",
    "local",
    "inherited",
    "default style trigger",
    "template trigger",
    "style trigger",
    "implicit style",
    "parent template",
    "parent template trigger",
    "animation",
    "coercion",
    "visual state",
];

/// At most this many elements per dump.
const LIMIT: usize = 16;

impl Session {
    /// Appends the properties of the elements `selectors` (separated by `;`) match to the dump
    /// at `path`. Each element is read on its own UI thread (the tree service refuses others).
    pub(super) fn dump_props(self: &Arc<Self>, path: &Path, selectors: &str) {
        let selectors: Vec<Selector> = selectors
            .split(';')
            .filter(|s| !s.trim().is_empty())
            .filter_map(|s| {
                s.parse()
                    .inspect_err(|e: &crate::selector::ParseError| log(&e.to_string()))
                    .ok()
            })
            .collect();
        let (picked, windows) = {
            let state = self.lock();
            let mut picked: HashMap<u32, Vec<(InstanceHandle, String)>> = HashMap::new();
            for (&h, element) in state.nodes.iter().filter(|(_, element)| {
                let at = At {
                    nodes: &state.nodes,
                    element,
                };
                selectors.iter().any(|s| s.matches(&at))
            }) {
                picked
                    .entry(element.thread)
                    .or_default()
                    .push((h, format!("{}#{}", element.kind, element.name)));
            }
            (picked, state.threads.clone())
        };
        for (thread, elements) in picked {
            let Some(&ui) = windows.get(&thread) else {
                continue;
            };
            let session = self.clone();
            let path = path.to_path_buf();
            post(thread, ui, move || {
                let mut out = String::new();
                for (handle, label) in elements.into_iter().take(LIMIT) {
                    // Only what is still in the tree: a handle that left may name another object.
                    if !session.lock().nodes.contains_key(&handle) {
                        continue;
                    }
                    let _ = writeln!(out, "\n== {label} ({handle})");
                    session.describe(handle, "  ", &mut out);
                }
                if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(&path) {
                    let _ = file.write_all(out.as_bytes());
                }
            });
        }
    }

    /// One line per value in `handle`'s property chain (defaults left out). Brush values get
    /// their own `Color` and `Opacity` below them.
    fn describe(&self, handle: InstanceHandle, indent: &str, out: &mut String) {
        let mut source_count = 0;
        let mut sources: *mut PropertyChainSource = std::ptr::null_mut();
        let mut value_count = 0;
        let mut values: *mut PropertyChainValue = std::ptr::null_mut();
        // SAFETY: valid out pointers; the arrays are freed below.
        let got = unsafe {
            self.tree.GetPropertyValuesChain(
                handle,
                &mut source_count,
                &mut sources,
                &mut value_count,
                &mut values,
            )
        };
        if let Err(e) = got.ok() {
            let _ = writeln!(out, "{indent}(no properties: {e})");
            return;
        }
        let mut nested = Vec::new();
        // SAFETY: the counts describe the arrays the call returned, freed once at the end.
        unsafe {
            for i in 0..value_count as usize {
                let v = &*values.add(i);
                let source = (v.PropertyChainIndex < source_count)
                    .then(|| &*sources.add(v.PropertyChainIndex as usize));
                let kind = source.map_or(0, |s| s.Source);
                if kind == 1 {
                    continue; // default
                }
                let name = text(v.PropertyName);
                let value = text(v.Value);
                let _ = writeln!(
                    out,
                    "{indent}{name} = {value} : {} (type {}) [{}{}{}]{}",
                    text(v.ValueType),
                    text(v.Type),
                    SOURCES.get(kind as usize).copied().unwrap_or("?"),
                    source
                        .map(|s| text(s.Name))
                        .filter(|n| !n.is_empty())
                        .map(|n| format!(" {n}"))
                        .unwrap_or_default(),
                    if v.Overridden.as_bool() {
                        ", overridden"
                    } else {
                        ""
                    },
                    if v.MetadataBits != 0 {
                        format!(" bits {:#x}", v.MetadataBits)
                    } else {
                        String::new()
                    },
                );
                if v.MetadataBits & IS_VALUE_HANDLE != 0
                    && text(v.ValueType).ends_with("Brush")
                    && let Ok(h) = value.parse::<InstanceHandle>()
                {
                    nested.push(h);
                }
            }
            free_chain(sources, source_count, values, value_count);
        }
        if indent.len() < 4 {
            for h in nested {
                self.describe(h, "      ", out);
            }
        }
    }
}
