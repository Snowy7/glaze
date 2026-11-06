//! Latest-value overrides. Durable revisions, cancellation and disconnect retire them.
use super::wire::{Document, PreviewDocument, PreviewOp, Revision};

#[derive(Default)]
pub struct Preview {
    session: Option<u64>,
    sequence: u64,
    values: Vec<Value>,
}

struct Value {
    document: PreviewDocument,
    sequence: u64,
    saved: Option<u64>,
}

impl Preview {
    pub fn has(&self, document: Document) -> bool {
        self.values.iter().any(|v| v.document.document == document)
    }
    fn accepts(&mut self, session: u64, sequence: u64) -> bool {
        if self.session.is_some_and(|old| old != session) || sequence <= self.sequence {
            return false;
        }
        self.session = Some(session);
        self.sequence = sequence;
        true
    }

    pub fn update(&mut self, session: u64, sequence: u64, documents: Vec<PreviewDocument>) {
        if documents.is_empty()
            || documents.len() > 3
            || documents.iter().enumerate().any(|(i, d)| {
                d.ops.is_empty()
                    || d.ops.len() > 32
                    || d.ops.iter().any(|op| !op.valid(d.document))
                    || documents[..i].iter().any(|old| old.document == d.document)
            })
            || !self.accepts(session, sequence)
        {
            return;
        }
        self.values = documents
            .into_iter()
            .map(|document| Value {
                document,
                sequence,
                saved: None,
            })
            .collect();
    }

    pub fn dock(&mut self, session: u64, sequence: u64, base: u64, size: f64) {
        if !size.is_finite() || !(24.0..=96.0).contains(&size) {
            return;
        }
        self.update(
            session,
            sequence,
            vec![PreviewDocument {
                document: Document::Bars,
                base_revision: base,
                ops: vec![PreviewOp {
                    path: "/dock/size".into(),
                    value: serde_json::json!(size),
                }],
            }],
        );
    }

    pub fn saved(&mut self, session: u64, sequence: u64, revisions: &[Revision]) {
        if !self.accepts(session, sequence) {
            return;
        }
        for value in &mut self.values {
            value.saved = revisions
                .iter()
                .find(|r| r.document == value.document.document)
                .map(|r| r.revision);
        }
    }

    pub fn cancel(&mut self, session: u64, sequence: u64) {
        if self.accepts(session, sequence) {
            self.values.clear();
        }
    }

    /// Each file hands over independently: saves across files need not arrive together.
    pub fn effective(&mut self, document: Document, durable: u64) -> Option<(u64, &[PreviewOp])> {
        let index = self
            .values
            .iter()
            .position(|v| v.document.document == document)?;
        let value = &self.values[index];
        if value.saved == Some(durable) || value.document.base_revision != durable {
            // An external edit wins too. An old drag must never overwrite newer saved state.
            self.values.remove(index);
            return None;
        }
        let value = &self.values[index];
        Some((value.sequence, &value.document.ops))
    }
}

#[cfg(test)]
mod tests {
    use super::super::wire::patched;
    use super::*;
    use serde_json::json;

    fn document(
        document: Document,
        base_revision: u64,
        path: &str,
        value: serde_json::Value,
    ) -> PreviewDocument {
        PreviewDocument {
            document,
            base_revision,
            ops: vec![PreviewOp {
                path: path.into(),
                value,
            }],
        }
    }

    #[test]
    fn release_holds_each_document_until_its_matching_save() {
        let mut p = Preview::default();
        p.update(
            7,
            1,
            vec![
                document(Document::Config, 10, "/look/material/blur", json!(24)),
                document(Document::Bars, 11, "/look/glass/blur", json!(24)),
                document(Document::Widgets, 12, "/appearance/blur", json!(24)),
            ],
        );
        p.saved(
            7,
            2,
            &[
                Revision {
                    document: Document::Config,
                    revision: 20,
                },
                Revision {
                    document: Document::Bars,
                    revision: 21,
                },
                Revision {
                    document: Document::Widgets,
                    revision: 22,
                },
            ],
        );
        assert!(p.effective(Document::Config, 10).is_some());
        assert!(p.effective(Document::Config, 20).is_none());
        assert!(p.effective(Document::Bars, 11).is_some());
        assert!(p.effective(Document::Bars, 21).is_none());
        assert!(p.effective(Document::Widgets, 12).is_some());
        assert!(p.effective(Document::Widgets, 22).is_none());
        p.dock(7, 1, 11, 72.0);
        assert!(p.effective(Document::Bars, 21).is_none());
    }

    #[test]
    fn stale_sessions_conflicts_cancel_and_disconnect_cannot_reopen_overrides() {
        let mut p = Preview::default();
        p.dock(7, 1, 10, 48.0);
        p.dock(7, 2, 10, 70.0);
        p.dock(7, 1, 10, 30.0);
        p.dock(8, 3, 10, 80.0);
        p.dock(7, 3, 10, f64::NAN);
        assert_eq!(
            patched(&json!({}), p.effective(Document::Bars, 10).unwrap().1).unwrap()["dock"]["size"],
            70.0
        );
        assert!(p.effective(Document::Bars, 11).is_none());
        p.dock(7, 3, 11, 60.0);
        p.cancel(7, 4);
        p.dock(7, 3, 11, 48.0);
        assert!(p.effective(Document::Bars, 11).is_none());
        p = Preview::default();
        p.dock(8, 1, 11, 60.0);
        assert!(p.effective(Document::Bars, 11).is_some());
    }

    #[test]
    fn invalid_materials_and_non_preview_paths_are_rejected_atomically() {
        let mut p = Preview::default();
        for (doc, path, value) in [
            (Document::Config, "/look/material/blur", json!(33)),
            (Document::Config, "/look/material/refraction", json!(-1)),
            (
                Document::Config,
                "/look/material/tint",
                json!("not a color"),
            ),
            (Document::Config, "/theme", json!("other")),
            (Document::Widgets, "/instances/0/size/0", json!(2.5)),
        ] {
            p.update(7, 1, vec![document(doc, 10, path, value)]);
            assert!(p.effective(doc, 10).is_none());
        }
        p.update(
            7,
            1,
            vec![document(
                Document::Config,
                10,
                "/look/material/highlight",
                json!(0.7),
            )],
        );
        assert!(p.effective(Document::Config, 10).is_some());
    }

    #[test]
    fn patches_preserve_unrelated_state_and_reject_missing_widget_indices() {
        let raw = json!({"instances":[{"size":[8,5],"cell":[1,2],"appearance":{"tint":"#123456"}}], "grid":{"gap":8}});
        let ops = [
            PreviewOp {
                path: "/instances/0/appearance/blur".into(),
                value: json!(20),
            },
            PreviewOp {
                path: "/instances/0/size/0".into(),
                value: json!(10),
            },
        ];
        let changed = patched(&raw, &ops).unwrap();
        assert_eq!(changed["instances"][0]["size"], json!([10, 5]));
        assert_eq!(changed["instances"][0]["appearance"]["tint"], "#123456");
        assert_eq!(raw["instances"][0]["size"], json!([8, 5]));
        assert!(
            patched(
                &raw,
                &[PreviewOp {
                    path: "/instances/7/size/0".into(),
                    value: json!(10)
                }]
            )
            .is_none()
        );
    }

    #[test]
    fn choices_replace_each_other_and_leave_or_matching_save_restores_the_base() {
        let mut preview = Preview::default();
        let raw = json!({"top":{"design":"monochrome"},"dock":{"pinned":["keep.exe"]},
            "overrides":{"DISPLAY2":{"dock":{"design":"minimal"}}}});
        let revision = super::super::wire::revision(&raw);
        for (sequence, design) in [(1, "macos"), (2, "fluent"), (3, "capsule")] {
            preview.update(7, sequence, vec![document(Document::Bars, revision,
                "/overrides/DISPLAY2/top/design", json!(design))]);
            let effective = patched(&raw, preview.effective(Document::Bars, revision).unwrap().1).unwrap();
            assert_eq!(effective["overrides"]["DISPLAY2"]["top"]["design"], design);
            assert_eq!(effective["top"]["design"], "monochrome");
            assert_eq!(effective["dock"]["pinned"], raw["dock"]["pinned"]);
        }
        preview.cancel(7, 4);
        assert!(preview.effective(Document::Bars, revision).is_none());
        preview.update(7, 5, vec![document(Document::Widgets, 9,
            "/instances/0/settings/variant", json!("digital"))]);
        assert!(!preview.has(Document::Bars));
        preview.saved(7, 6, &[Revision { document: Document::Widgets, revision: 10 }]);
        assert!(preview.effective(Document::Widgets, 9).is_some());
        assert!(preview.effective(Document::Widgets, 10).is_none());
        assert_eq!(raw["top"]["design"], "monochrome");
    }

    #[test]
    fn choice_validation_excludes_commands_sources_and_windows_styling() {
        for (doc, path, value) in [
            (Document::Config, "/look/windows/backdrop", json!("mica")),
            (Document::Bars, "/overrides/DISPLAY2/modules/clock/click", json!("run")),
            (Document::Bars, "/overrides/DISPLAY2/taskbar/mode", json!("hide")),
            (Document::Bars, "/top/design", json!("unknown")),
            (Document::Widgets, "/instances/0/settings/target", json!("run.exe")),
            (Document::Widgets, "/instances/0/appearance/radius", json!(49)),
        ] {
            assert!(!super::super::wire::PreviewOp { path: path.into(), value }.valid(doc), "{path}");
        }
    }
}
