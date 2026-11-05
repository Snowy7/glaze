//! Uses the production shell renderer; no network, pipe, credential or vendor side effects.
use super::*;

#[test]
fn discord_plugin_size_sheet_and_native_action_hits() {
    use views::plugin::{ACTION, Hit, action_at};
    let faces: Value = serde_json::from_str(include_str!("../golden/faces.json")).unwrap();
    let bot: mypaper_plugin_sdk::Face = serde_json::from_value(faces["bot"]["large"].clone()).unwrap();
    assert!(matches!(action_at(&bot,ACTION+3),Some(Hit::Select {action,..}) if action=="channel"));
    assert_eq!(action_at(&bot,ACTION+4),Some(Hit::Field("send".into())));
    let readonly: mypaper_plugin_sdk::Face = serde_json::from_value(faces["readonly"]["large"].clone()).unwrap();
    let mut ui = Ui::new().unwrap();
    let mut sheet = DisplayList::default();
    sheet.ops.push(Op::Fill { rect:Rect::new(0.0,0.0,1100.0,1120.0),radius:0.0,color:Rgba::rgb8([22,29,40],1.0) });
    for (id,class,size,x,y) in [
        ("messages","small",(180,180),20.0,50.0),
        ("messages","medium",(360,225),220.0,50.0),
        ("voice","large",(450,360),600.0,50.0),
        ("setup","medium",(360,300),20.0,460.0),
        ("voice","small",(180,180),410.0,460.0),
        ("bot","medium",(360,300),700.0,460.0),
        ("empty","small",(180,180),20.0,830.0),
        ("bot","small",(180,180),220.0,830.0),
        ("readonly","large",(360,240),450.0,830.0),
    ] {
        sheet.ops.push(Op::Text {rect:Rect::new(x,y-30.0,size.0 as f32,24.0),text:format!("{id} / {class}").into(),style:TextStyle::new("Segoe UI Variable",13.0),color:Rgba::rgb8([210,222,238],1.0)});
        let mut m = model(&faces[id][class],"Discord");
        m.data["icon"]="message".into();
        let list = draw(&mut ui,&m,size);
        assert!(list.damage(&draw(&mut ui,&m,size)).is_none());
        for hit in &list.hits {
            assert!(hit.rect.left>=0.0 && hit.rect.top>=0.0 && hit.rect.right<=size.0 as f32+0.5 && hit.rect.bottom<=size.1 as f32+0.5,"{id}/{class}: {} {:?}",hit.label,hit.rect);
            if id=="readonly" && (hit.label.as_ref()=="Mute" || hit.label.as_ref()=="Deafen") { assert!(!hit.enabled); }
        }
        if id=="bot" { assert!(list.hits.iter().any(|h|h.label.contains("Send as bot")),"composer missing at {class}"); }
        let rect = Rect::new(x,y,size.0 as f32,size.1 as f32);
        sheet.ops.push(Op::Fill {rect,radius:22.0,color:Rgba::rgb8([44,52,64],1.0)});
        sheet.ops.push(Op::Image {rect,image:Arc::new(render(&mut ui,&list,size.0,size.1).unwrap()),opacity:1.0,source:None});
    }
    assert!(serde_json::to_string(&readonly).unwrap().contains("\"enabled\":false"));
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/discord/golden/sizes.png");
    golden::check(&render(&mut ui,&sheet,1100,1120).unwrap(),&path).unwrap();
}
