//! One latest-value dock-size override, tied to a Studio session and a durable revision.
#[derive(Default)]
pub(super) struct Preview {
    session: Option<u64>,
    sequence: u64,
    value: Option<Value>,
}

struct Value {
    base: u64,
    size: f64,
    saved: Option<u64>,
}

impl Preview {
    pub fn active(&self) -> bool {
        self.value.is_some()
    }

    fn accepts(&mut self, session: u64, sequence: u64) -> bool {
        if self.session.is_some_and(|old| old != session) || sequence <= self.sequence {
            return false;
        }
        self.session = Some(session);
        self.sequence = sequence;
        true
    }

    pub fn update(&mut self, session: u64, sequence: u64, base: u64, size: f64) {
        if size.is_finite() && (24.0..=96.0).contains(&size) && self.accepts(session, sequence) {
            self.value = Some(Value {
                base,
                size,
                saved: None,
            });
        }
    }

    pub fn saved(&mut self, session: u64, sequence: u64, revision: u64) {
        if self.accepts(session, sequence)
            && let Some(value) = &mut self.value
        {
            value.saved = Some(revision);
        }
    }

    pub fn cancel(&mut self, session: u64, sequence: u64) {
        if self.accepts(session, sequence) {
            self.value = None;
        }
    }

    pub fn effective(&mut self, durable: u64) -> Option<f64> {
        let value = self.value.as_ref()?;
        if value.saved == Some(durable) || value.base != durable {
            // Matching save takes over without bouncing; an external edit wins on conflict.
            self.value = None;
            None
        } else {
            Some(value.size)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_holds_until_matching_save_and_old_messages_cannot_reopen_it() {
        let mut p = Preview::default();
        p.update(7, 1, 10, 48.0);
        p.update(7, 2, 10, 70.0);
        p.update(7, 1, 10, 30.0);
        assert_eq!(p.effective(10), Some(70.0));
        p.saved(7, 3, 20);
        assert_eq!(p.effective(10), Some(70.0));
        assert_eq!(p.effective(20), None);
        p.update(7, 2, 10, 70.0);
        assert_eq!(p.effective(20), None);
        p.update(7, 4, 20, 50.0);
        assert_eq!(p.effective(20), Some(50.0));
    }
    #[test]
    fn conflicts_cancel_disconnect_and_invalid_values_drop_transient_state() {
        let mut p = Preview::default();
        p.update(7, 1, 10, 48.0);
        assert_eq!(p.effective(11), None);
        p.update(7, 2, 11, 64.0);
        p.update(8, 3, 11, 80.0);
        p.update(7, 3, 11, f64::NAN);
        assert_eq!(p.effective(11), Some(64.0));
        p.cancel(7, 4);
        assert_eq!(p.effective(11), None);
        p.update(7, 3, 11, 48.0);
        assert_eq!(p.effective(11), None);
        p = Preview::default();
        p.update(8, 1, 11, 60.0);
        assert_eq!(p.effective(11), Some(60.0));
    }
}
