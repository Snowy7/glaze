//! Executes the shipped WASM through the host; request effects are inspected, never dispatched.
use crate::{caps, runtime::{Effect, Manifest, Runtime}};
use crate::runtime::exports::mypaper::plugin::lifecycle::{Action, Event, FaceOpen};
use crate::runtime::mypaper::plugin::{http::{Reply, Response}, types::SizeClass};
use serde_json::json;

#[test]
fn grok_wasm_brokers_routines_and_keeps_drafts_explicit() -> anyhow::Result<()> {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/grok");
    let mut manifest = Manifest::load(&dir)?;
    let scratch = std::env::temp_dir().join(format!("mypaper-grok-wasm-{}",std::process::id()));
    let runtime = Runtime::new(scratch.join("cache"),scratch.join("data"))?;
    let (component,_) = runtime.component(&manifest,&dir)?;
    let id = format!("wasm-test-{}",std::process::id());
    let settings = json!({"routines":[{"id":id,"title":"Fixture routine","url":"https://example.test/run","input":{"explicit":true}}]}).to_string();
    let mut instance = runtime.instantiate_with_settings(&manifest,&component,None,&settings)?;
    let open=|face: &str| Event::Open(FaceOpen {instance:face.into(),face:face.into(),size:SizeClass::Medium,settings:settings.clone()});
    let action=|instance: &str,name: &str,value: Option<&str>| Event::Action(Action {instance:instance.into(),action:name.into(),value:value.map(str::to_owned)});
    for face in ["routines","recent","prompt"] {
        let effects = instance.event(&open(face))?;
        assert!(effects.iter().any(|e| matches!(e,Effect::View(id,_) if id==face)),"missing {face} view");
        assert!(!effects.iter().any(|e| matches!(e,Effect::Fetch {..})),"opening a face sent a webhook");
    }
    let effects=instance.event(&action("prompt","prompt",Some(" ")))?;
    assert!(effects.iter().any(|e|matches!(e,Effect::View(_,face) if face.footer.as_deref().is_some_and(|f|f.contains("shorter prompt")))));
    let secret_key=format!("grok/{id}");
    caps::secret_set("grok",&secret_key,"fixture-key")?;
    let result=(|| -> anyhow::Result<()> {
        let effects=instance.event(&action("routines",&format!("run:{id}"),None))?;
        let request=effects.iter().find_map(|e|match e {Effect::Fetch {id,method,url,headers,body,..}=>Some((*id,*method,url,headers,body)),_=>None}).expect("brokered POST");
        assert_eq!((request.1,request.2.as_str()),("POST","https://example.test/run"));
        assert!(request.3.contains(&("Authorization".into(),"Bearer fixture-key".into())));
        assert_eq!(serde_json::from_slice::<serde_json::Value>(request.4.as_deref().unwrap())?,json!({"explicit":true}));
        assert!(!instance.event(&action("routines",&format!("run:{id}"),None))?.iter().any(|e|matches!(e,Effect::Fetch {..})),"duplicate in-flight request");
        let effects=instance.event(&Event::Http(Reply{id:request.0,outcome:Ok(Response{status:200,headers:vec![],body:vec![]})}))?;
        assert!(effects.iter().any(|e|matches!(e,Effect::View(_,face) if serde_json::to_string(face).unwrap().contains("Accepted"))));
        let saved=instance.save()?;
        assert!(!String::from_utf8_lossy(&saved).contains("fixture-key"));
        manifest.permissions.retain(|p|!p.starts_with("network:"));
        let mut denied=runtime.instantiate_with_settings(&manifest,&component,None,&settings)?;
        denied.event(&open("routines"))?;
        let effects=denied.event(&action("routines",&format!("run:{id}"),None))?;
        assert!(!effects.iter().any(|e|matches!(e,Effect::Fetch {..})));
        Ok(())
    })();
    caps::secret_remove("grok",&secret_key)?;
    drop(instance); drop(component); drop(runtime);
    let _=std::fs::remove_dir_all(&scratch);
    result
}
