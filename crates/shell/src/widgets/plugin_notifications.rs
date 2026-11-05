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
    replay: bool,
}
fn card(c: &Card) -> Value {
    json!({"id":c.id,"appId":c.app_id,"app":c.app,"title":c.title,"body":c.body,"at":c.at})
}
fn changes(old: &BTreeMap<u32, Card>, new: &BTreeMap<u32, Card>, replay: bool) -> Vec<Value> {
    let mut output = Vec::new();
    for id in old.keys().filter(|id|!new.contains_key(id)) { output.push(json!({"kind":"removed","id":id})); }
    for (id,c) in new.iter().filter(|(id,c)|replay || old.get(id)!=Some(c)) { output.push(json!({"kind":"added","id":id,"card":card(c)})); }
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
        self.replay |= self.subscribers!=subscribers || self.lease.is_none();
        if self.subscribers!=subscribers {self.revision=None;self.subscribers=subscribers;}
        if self.lease.is_none() {self.lease=crate::integrations::acquire(Provider::Notifications).ok();}
        let Some(snapshot) = self.lease.as_ref().map(Lease::snapshot) else {return};
        if self.revision==Some(snapshot.revision) {return;}
        self.revision=Some(snapshot.revision);
        let Some(Data::Notifications(snapshot)) = snapshot.data else {return};
        let cards: BTreeMap<_,_> = snapshot.cards.into_iter().map(|c|(c.id,c)).collect();
        // Keep identities across a released lease to deliver removals during a hold. Replay
        // stays pending through the asynchronous listener's initial Connecting snapshot.
        let outgoing=changes(&self.cards,&cards,self.replay);
        self.replay=false;
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
        let added = changes(&BTreeMap::new(),&first,false);
        assert_eq!(added[0]["kind"],"added");
        assert_eq!(added[0]["card"]["appId"],"chat.app");
        assert_eq!(added[0]["card"]["body"],"");
        assert!(changes(&first,&first,false).is_empty());
        assert_eq!(changes(&first,&first,true),added,"new subscribers replay unchanged retained cards");
        let changed = BTreeMap::from([(7,Card{title:"Updated".into(),..c})]);
        assert_eq!(changes(&first,&changed,false)[0]["card"]["title"],"Updated");
        assert_eq!(changes(&changed,&BTreeMap::new(),true),vec![json!({"kind":"removed","id":7})]);
    }
}
