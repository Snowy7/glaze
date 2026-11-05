//! Notification delivery reuses the shell's one reference-counted listener. No timer or polling.
use mypaper_integrations::{Data, Lease, Provider, notifications::Card};
use mypaper_plugin_api::wire::SystemEvent;
use mypaper_widgets::plugins::Plugins;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Notifications {
    lease: Option<Lease>,
    generation: Option<u64>,
    subscribers: Vec<String>,
    cards: BTreeMap<u32, Card>,
    revision: Option<u64>,
}
fn card(c: &Card) -> Value {
    json!({"id":c.id,"appId":c.app_id,"app":c.app,"title":c.title,"body":c.body,"at":c.at})
}
fn changes(old: &BTreeMap<u32, Card>, new: &BTreeMap<u32, Card>) -> Vec<Value> {
    let mut output = Vec::new();
    for id in old.keys().filter(|id|!new.contains_key(id)) { output.push(json!({"kind":"removed","id":id})); }
    for (id,c) in new.iter().filter(|(id,c)|old.get(id)!=Some(c)) { output.push(json!({"kind":"added","id":id,"card":card(c)})); }
    output
}
impl Notifications {
    pub(super) fn sync(&mut self, plugins: &Plugins, visible: impl Fn(&str)->bool) {
        let subscribers: Vec<String> = plugins.notification_instances().into_iter().filter(|i|visible(i)).map(str::to_owned).collect();
        let generation = crate::integrations::generation();
        if subscribers.is_empty() || self.generation!=Some(generation) {
            self.lease=None; self.revision=None;
        }
        self.generation=Some(generation);
        if subscribers.is_empty() {self.subscribers.clear();return;}
        let replay = self.subscribers!=subscribers || self.lease.is_none();
        if self.subscribers!=subscribers {self.revision=None;self.subscribers=subscribers;}
        if self.lease.is_none() {self.lease=crate::integrations::acquire(Provider::Notifications).ok();}
        let Some(snapshot) = self.lease.as_ref().map(Lease::snapshot) else {return};
        if self.revision==Some(snapshot.revision) {return;}
        self.revision=Some(snapshot.revision);
        let Some(Data::Notifications(snapshot)) = snapshot.data else {return};
        let cards: BTreeMap<_,_> = snapshot.cards.into_iter().map(|c|(c.id,c)).collect();
        let mut outgoing=changes(&self.cards,&cards);
        if replay {
            // Keep the last identities across a released lease, so removals during a hold
            // reach returning subscribers too. New subscribers also receive every retained card.
            outgoing.retain(|event|event["kind"]=="removed");
            outgoing.extend(cards.values().map(|c|json!({"kind":"added","id":c.id,"card":card(c)})));
        }
        for event in outgoing {plugins.system_event(SystemEvent::Notification(event.to_string()));}
        self.cards=cards;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notification_add_update_remove_preserves_identity_and_privacy() {
        let c = Card{id:7,app_id:"chat.app".into(),app:"Chat".into(),title:"".into(),body:"".into(),at:123};
        let first = BTreeMap::from([(7,c.clone())]);
        let added = changes(&BTreeMap::new(),&first);
        assert_eq!(added[0]["kind"],"added");
        assert_eq!(added[0]["card"]["appId"],"chat.app");
        assert_eq!(added[0]["card"]["body"],"");
        assert!(changes(&first,&first).is_empty());
        let changed = BTreeMap::from([(7,Card{title:"Updated".into(),..c})]);
        assert_eq!(changes(&first,&changed)[0]["card"]["title"],"Updated");
        assert_eq!(changes(&changed,&BTreeMap::new()),vec![json!({"kind":"removed","id":7})]);
    }
}
