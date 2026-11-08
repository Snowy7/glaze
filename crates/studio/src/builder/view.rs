use mypaper_ui::{Rect, Role, TextStyle};
use mypaper_ui::controls::{Align, Behavior, key};
use serde_json::{Value, json};
use crate::{actions::{Action, FieldId}, app::App, panel::{Page, controls::{self, text, title, group, button, notice}, editors}, theme::glyph, ui::{B, Col}};
use super::{Command, active, raw, edit};

pub fn action(command: Command) -> Action { Action::Panel(crate::panel::Command::Builder(command)) }
fn open(page: Page) -> Action { Action::Panel(crate::panel::Command::Open(page)) }
fn line(b: &mut B, c: &mut Col, label: &str, detail: &str, action: Action) {
    controls::row(b,c,None,label,detail,"",None,action);
}
fn text_field(app: &App,b: &mut B,c: &mut Col,path: String,label: &str,value: &str) {
    editors::field(app,b,c,FieldId::Text(active(app).unwrap().doc,path.into()),label,"",value);
}
fn slider(app: &App,b: &mut B,c: &mut Col,path: String,label: &str,value: f64,min: f64,max: f64,step: f64) {
    let h=b.px(22.0);
    text(b,Rect::new(c.x,c.y,c.w,h),label,12.0,500,b.p.ink2,Align::Start);
    c.y+=h;
    let r=Rect::new(c.x,c.y,c.w,b.px(32.0));
    let shown=if step>=1.0 {format!("{value:.0}")}else{format!("{value:.2}")};
    b.slider(key(("builder-slider",&path)),r,value,(min,max,step),&shown,b.px(45.0),label,Action::Slider(edit(app,path,label)));
    c.y=r.bottom+b.px(7.0);
}
fn options(app: &App,b: &mut B,c: &mut Col,path: String,label: &str,value: Value,values: &[&str]) {
    editors::select(b,c,label,value,edit(app,path,label),values.iter().map(|v|(json!(v),v.to_string())).collect());
}
fn color(app: &App,b: &mut B,c: &mut Col,path: String,label: &str,value: &Value) {
    let hex=value.as_str().unwrap_or("#ffffff");
    let r=Rect::new(c.x,c.y,c.w,b.px(38.0));
    text(b,Rect::new(r.left,r.top,r.width()-b.px(40.0),r.height()),label,13.0,500,b.p.ink1,Align::Start);
    let swatch=Rect::new(r.right-b.px(30.0),r.top+b.px(6.0),b.px(26.0),b.px(26.0));
    let fill=mypaper_config::Color::parse(hex).map(|v|mypaper_ui::Rgba::rgb8(v.0,1.0)).unwrap_or(b.p.ink1);
    b.fill(swatch,b.px(8.0),fill);
    b.stroke(swatch,b.px(8.0),b.px(1.0),b.p.edge);
    b.hit(key(("builder-color",&path)),r,Role::Button,&format!("{label} {hex}"),Action::Panel(crate::panel::Command::PickColor(edit(app,path,label))),Behavior::Click);
    c.y=r.bottom;
}
fn pair(b: &mut B,c: &mut Col,left: (&str,Action),right: (&str,Action)) {
    let gap=b.px(8.0); let w=(c.w-gap)/2.0; let h=b.px(34.0);
    button(b,Rect::new(c.x,c.y,w,h),left.0,None,false,true,left.1);
    button(b,Rect::new(c.x+w+gap,c.y,w,h),right.0,None,false,true,right.1);
    c.y+=h+b.px(10.0);
}

pub fn page(app: &App,b: &mut B,c: &mut Col,page: Page) {
    let Some(editor)=active(app) else {title(b,c,"Create wallpaper","");line(b,c,"Start creating","",action(Command::New));return;};
    let value=raw(app);
    if page==Page::Effects {
        title(b,c,"Effects","A little atmosphere. Your composition.");
        group(b,c,"Browse",|b,c|{
            line(b,c,"All effects",&editor.effects.len().to_string(),action(Command::Category(None)));
            for category in ["weather","camera","color","motion","stylize","light"] {
                let count=editor.effects.iter().filter(|e|e.category()==category).count();
                line(b,c,&capitalize(category),&count.to_string(),action(Command::Category(Some(category.into()))));
            }
        });
        group(b,c,"Your own",|b,c|{
            line(b,c,"Image or 3D layer","Import",action(Command::Import(true)));
            line(b,c,"Write an effect","WGSL",action(Command::CustomEffect));
        });
        c.y+=b.px(16.0);line(b,c,"Back to composition","",open(Page::Create));
        return;
    }
    if page==Page::Shader {
        title(b,c,"Shader","Changes appear as you type.");
        let filename=editor.shader.as_ref().and_then(|p|p.file_name()).map(|n|n.to_string_lossy().into_owned()).unwrap_or_default();
        group(b,c,"Source",|b,c|{
            notice(b,c,&filename);
            line(b,c,"Edit in external editor","Open",action(Command::External));
            line(b,c,"Back to composition","",open(Page::Create));
        });
        group(b,c,"Diagnostics",|b,c|{
            if editor.diagnostics.is_empty() {notice(b,c,"No shader errors reported");}
            for error in &editor.diagnostics {notice(b,c,&format!("Line {} · {}",error.line,error.message));}
        });
        let schemas=super::shader_params(&editor.source);
        if !schemas.is_empty() {group(b,c,"Parameters",|b,c|{let prefix=param_prefix(editor.selected);params(app,b,c,&value,&prefix,&schemas);});}
        if value["wallpaper"]["base"]["kind"]=="glsl" && editor.selected.is_none() {
            group(b,c,"Shader credits",|b,c|{for (id,label) in [("author","Author"),("license","License"),("url","Source URL")] {text_field(app,b,c,format!("/wallpaper/base/{id}"),label,value["wallpaper"]["base"][id].as_str().unwrap_or(""));}});
        }
        return;
    }
    title(b,c,"Create","Make a wallpaper that feels like yours.");
    pair(b,c,("Save",action(Command::Save(false))),("Save & apply",action(Command::Save(true))));
    text_field(app,b,c,"/name".into(),"Name",value["name"].as_str().unwrap_or(""));
    let tab=Rect::new(c.x,c.y,c.w,b.px(32.0));
    b.segmented(key("builder-panel-tabs"),tab,&["Composition","Parameters"],Some(usize::from(editor.selected.is_some())),"Editor",true,|i|if i==0 {action(Command::Select(None))}else{action(Command::Select(Some(0)))});
    c.y=tab.bottom+b.px(6.0);
    if let Some(index)=editor.selected && let Some(layer)=value["wallpaper"]["layers"].get(index) {
        layer_params(app,b,c,index,layer);
    } else {
        base(app,b,c,&value["wallpaper"]["base"]);
        group(b,c,"Layers",|b,c|{
            let layers=super::layers(app);
            if layers.is_empty(){notice(b,c,"Add atmosphere, color or movement.");}
            for (i,layer) in layers.iter().enumerate() {
                let r=Rect::new(c.x,c.y,c.w,b.px(46.0));
                let handle=Rect::new(c.x,r.top,b.px(24.0),r.height());
                b.icon(handle,glyph::GRIP,13.0,b.p.ink3);
                b.hit(key(("builder-drag",i)),handle,Role::Button,"Drag layer to reorder",action(Command::Drag(i)),Behavior::Drag);
                let label=layer_name(app,layer);
                text(b,Rect::new(r.left+b.px(30.0),r.top,r.width()-b.px(72.0),r.height()),&label,13.0,500,b.p.ink1,Align::Start);
                b.hit(key(("builder-layer",i)),Rect::new(r.left+b.px(25.0),r.top,r.width()-b.px(65.0),r.height()),Role::Button,&format!("Edit {label}"),action(Command::Select(Some(i))),Behavior::Click);
                let on=layer["enabled"].as_bool().unwrap_or(true);
                let toggle=Rect::new(r.right-b.px(34.0),r.top+b.px(10.0),b.px(32.0),b.px(24.0));
                b.switch(key(("builder-enabled",i)),mypaper_ui::Point::new(toggle.left,toggle.top),on,"Enable layer",Action::Toggle(edit(app,format!("/wallpaper/layers/{i}/enabled"),"Enable layer"),on));
                c.y=r.bottom;
            }
            c.y+=b.px(8.0);
            pair(b,c,("Add effect",action(Command::Gallery)),("Add media",action(Command::Import(true))));
        });
        group(b,c,"Playback",|b,c|{
            slider(app,b,c,"/wallpaper/fps".into(),"Frame rate",value["wallpaper"]["fps"].as_f64().unwrap_or(30.0),1.0,120.0,1.0);
            slider(app,b,c,"/wallpaper/scale".into(),"Render scale",value["wallpaper"]["scale"].as_f64().unwrap_or(1.0),0.25,1.0,0.05);
            notice(b,c,&super::cost_line(app));
        });
    }
}

fn base(app:&App,b:&mut B,c:&mut Col,value:&Value) {
    let kind=value["kind"].as_str().unwrap_or("color");
    group(b,c,"Base",|b,c|{
        line(b,c,&capitalize(kind),value["source"].as_str().and_then(|s|s.rsplit('/').next()).unwrap_or(""),Action::None);
        pair(b,c,("Import file",action(Command::Import(false))),("Shader",action(Command::Base("shader"))));
        pair(b,c,("Color",action(Command::Base("color"))),("Gradient",action(Command::Base("gradient"))));
        match kind {
            "color"=>color(app,b,c,"/wallpaper/base/color".into(),"Color",&value["color"]),
            "image"|"video"=>{
                options(app,b,c,"/wallpaper/base/fit".into(),"Fit",value.get("fit").cloned().unwrap_or(json!("cover")),&["cover","contain","stretch","center"]);
                if kind=="image" { for (i,label) in ["Focus X","Focus Y"].into_iter().enumerate(){slider(app,b,c,format!("/wallpaper/base/focus/{i}"),label,value["focus"][i].as_f64().unwrap_or(0.5),0.0,1.0,0.01);} }
            }
            "gradient"=>{
                options(app,b,c,"/wallpaper/base/type".into(),"Shape",value["type"].clone(),&["linear","radial"]);
                if let Some(stops)=value["stops"].as_array(){for (i,stop) in stops.iter().enumerate(){color(app,b,c,format!("/wallpaper/base/stops/{i}/color"),&format!("Stop {}",i+1),&stop["color"]);slider(app,b,c,format!("/wallpaper/base/stops/{i}/at"),"Position",stop["at"].as_f64().unwrap_or(0.0),0.0,1.0,0.01);}}
                slider(app,b,c,"/wallpaper/base/angle".into(),"Angle",value["angle"].as_f64().unwrap_or(120.0),0.0,360.0,1.0);
            }
            "shader"|"glsl"=>{line(b,c,"Open shader editor","Code",action(Command::Shader));let source=active(app).unwrap().dir.join(value["source"].as_str().unwrap_or(""));if let Ok(source)=std::fs::read_to_string(source){params(app,b,c,&raw(app),"/wallpaper/base/params",&super::shader_params(&source));}},
            "scene"=>{
                slider(app,b,c,"/wallpaper/base/orbit".into(),"Orbit",value["orbit"].as_f64().unwrap_or(0.0),0.0,30.0,0.1);
                slider(app,b,c,"/wallpaper/base/parallax".into(),"Parallax",value["parallax"].as_f64().unwrap_or(0.3),0.0,1.0,0.01);
                slider(app,b,c,"/wallpaper/base/exposure".into(),"Exposure",value["exposure"].as_f64().unwrap_or(1.0),0.0,5.0,0.05);
                options(app,b,c,"/wallpaper/base/environment".into(),"Environment",value["environment"].clone(),&["sky","color","transparent"]);
            }
            _=>{},
        }
    });
}

fn param_prefix(index:Option<usize>)->String {index.map(|i|format!("/wallpaper/layers/{i}/params")).unwrap_or_else(||"/wallpaper/base/params".into())}
fn layer_name(app:&App,layer:&Value)->String {
    // The contract's id is also the editable layer label; no extra name field is written.
    layer["id"].as_str().filter(|s|!s.contains('-') || layer["effect"].as_str().is_none()).map(str::to_owned)
        .or_else(||active(app)?.effects.iter().find(|e|layer["effect"].as_str()==Some(&e.id)).map(|e|e.name.clone()))
        .unwrap_or_else(||layer["id"].as_str().unwrap_or("Layer").to_owned())
}
fn layer_params(app:&App,b:&mut B,c:&mut Col,index:usize,layer:&Value) {
    let prefix=format!("/wallpaper/layers/{index}");
    group(b,c,&layer_name(app,layer),|b,c|{
        text_field(app,b,c,format!("{prefix}/id"),"Layer name",layer["id"].as_str().unwrap_or(""));
        pair(b,c,("Move up",action(Command::Move(-1))),("Move down",action(Command::Move(1))));
        pair(b,c,("Duplicate",action(Command::Duplicate)),("Delete",action(Command::Remove)));
        editors::toggle(b,c,"Enabled","",layer["enabled"].as_bool().unwrap_or(true),Action::Toggle(edit(app,format!("{prefix}/enabled"),"Enable layer"),layer["enabled"].as_bool().unwrap_or(true)));
        slider(app,b,c,format!("{prefix}/mix"),"Mix",layer["mix"].as_f64().unwrap_or(1.0),0.0,1.0,0.01);
        options(app,b,c,format!("{prefix}/blend"),"Blend",layer.get("blend").cloned().unwrap_or(json!("normal")),&["normal","add","screen","multiply"]);
    });
    if layer["kind"]=="effect" {
        if let Some(effect)=active(app).unwrap().effects.iter().find(|e|layer["effect"].as_str()==Some(&e.id)) {
            group(b,c,"Parameters",|b,c|{params(app,b,c,&raw(app),&format!("{prefix}/params"),&effect.schemas());line(b,c,"Edit effect source","WGSL",action(Command::Shader));});
        }else{notice(b,c,"This effect package is missing. The layer is preserved.");}
    }else{
        group(b,c,"Transform",|b,c|{
            for (i,label) in ["Position X","Position Y","Width","Height"].into_iter().enumerate(){slider(app,b,c,format!("{prefix}/rect/{i}"),label,layer["rect"][i].as_f64().unwrap_or(0.5),0.0,1.0,0.01);}
            if layer["kind"]=="model" {
                for (id,label,default,max) in [("orbit","Orbit",0.0,30.0),("parallax","Parallax",0.3,1.0),("scale","Model scale",1.0,5.0)] {slider(app,b,c,format!("{prefix}/{id}"),label,layer[id].as_f64().unwrap_or(default),0.0,max,0.01);}
                for (i,label) in ["Rotation X","Rotation Y","Rotation Z"].into_iter().enumerate(){slider(app,b,c,format!("{prefix}/rotation/{i}"),label,layer["rotation"][i].as_f64().unwrap_or(0.0),-180.0,180.0,1.0);}
                for (i,label) in ["Light X","Light Y","Light Z"].into_iter().enumerate(){slider(app,b,c,format!("{prefix}/light/{i}"),label,layer["light"][i].as_f64().unwrap_or(0.5),-1.0,1.0,0.01);}
                text_field(app,b,c,format!("{prefix}/animation"),"Animation clip",layer["animation"].as_str().unwrap_or(""));
            }
        });
    }
}

fn params(app:&App,b:&mut B,c:&mut Col,value:&Value,prefix:&str,schemas:&[Value]) {
    for schema in schemas {
        let Some(id)=schema["id"].as_str() else{continue;};
        let label=schema["label"].as_str().unwrap_or(id);
        let path=format!("{prefix}/{}",id.replace('~',"~0").replace('/',"~1"));
        let own=value.pointer(&path);let current=own.unwrap_or(&schema["default"]);
        if own.is_some() && current!=&schema["default"] {
            let reset=Rect::new(c.right()-b.px(22.0),c.y,b.px(22.0),b.px(22.0));
            b.icon(reset,glyph::UNDO,12.0,b.p.ink3);
            b.hit(key(("builder-reset",&path)),reset,Role::Button,&format!("Reset {label}"),Action::Unset(edit(app,path.clone(),&format!("Reset {label}"))),Behavior::Click);
        }
        match schema["type"].as_str().unwrap_or("float") {
            "color"=>color(app,b,c,path,label,current),
            "bool"=>editors::toggle(b,c,label,"",current.as_bool().unwrap_or(false),Action::Toggle(edit(app,path,label),current.as_bool().unwrap_or(false))),
            "enum"=>{let options:Vec<_>=schema["options"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();options_fn(app,b,c,path,label,current.clone(),&options);},
            _=>slider(app,b,c,path,label,current.as_f64().unwrap_or(0.0),schema["min"].as_f64().unwrap_or(0.0),schema["max"].as_f64().unwrap_or(1.0),schema["step"].as_f64().unwrap_or(0.01)),
        }
    }
}
// A separate name avoids shadowing the generated enum options.
fn options_fn(app:&App,b:&mut B,c:&mut Col,path:String,label:&str,value:Value,values:&[&str]) {options(app,b,c,path,label,value,values);}

pub fn stage(app:&App,b:&mut B,area:Rect)->bool {
    let page=crate::panel::current_page(app);
    if !super::is_page(page){return false;}
    if active(app).is_none(){return true;}
    if page==Page::Effects {gallery(app,b,area);return true;}
    if page==Page::Shader {shader(app,b,area);return true;}
    let value=raw(app);let editor=active(app).unwrap();
    text(b,Rect::new(area.left+b.px(16.0),area.top+b.px(8.0),area.width()-b.px(32.0),b.px(24.0)),"CANVAS",11.0,600,b.p.ink3,Align::Start);
    let canvas=crate::stage::frame(app,Rect::new(area.left+b.px(16.0),area.top+b.px(60.0),area.width()-b.px(32.0),area.height()-b.px(145.0)));
    let snapshot=editor.dir.join("preview.jpg");
    let engine_frame=snapshot.is_file();
    let fallback=value["wallpaper"]["base"]["source"].as_str().map(|p|editor.dir.join(p)).filter(|p|crate::library::is_image(p));
    let path=engine_frame.then_some(snapshot).or(fallback);
    let image=path.as_ref().and_then(|p|app.images.borrow_mut().get(p,(canvas.width() as u32,canvas.height() as u32)));
    let fill=value["wallpaper"]["base"]["color"].as_str().and_then(mypaper_config::Color::parse).map(|v|mypaper_ui::Rgba::rgb8(v.0,1.0)).unwrap_or(b.p.well);
    b.fill(canvas,b.px(18.0),fill);
    if let Some(image)=image{crate::stage::image_round(b,canvas,&image,1.0,b.px(18.0));}
    b.stroke(canvas,b.px(18.0),b.px(1.0),b.p.edge);
    let status=if app.fixture || engine_frame {"Live preview"}else{"Base preview · waiting for engine"};
    text(b,Rect::new(canvas.left,canvas.bottom+b.px(18.0),canvas.width(),b.px(23.0)),status,12.0,500,b.p.ink2,Align::Center);
    text(b,Rect::new(canvas.left,canvas.bottom+b.px(42.0),canvas.width(),b.px(22.0)),&super::cost_line(app),11.0,400,b.p.ink3,Align::Center);
    let n=super::layers(app).len();
    text(b,Rect::new(area.left,area.bottom-b.px(36.0),area.width(),b.px(20.0)),&format!("{} · {n} layers · Draft",value["name"].as_str().unwrap_or("Untitled")),12.0,400,b.p.ink3,Align::Center);
    true
}

fn gallery(app:&App,b:&mut B,area:Rect) {
    let editor=active(app).unwrap();
    let k=key("builder-effect-gallery");
    b.scroll(k,area,app.nav.scroll(k),Action::None,|b,offset|{
        let mut c=Col{x:area.left+b.px(18.0),y:area.top+b.px(18.0)-offset,w:area.width()-b.px(36.0),view:k};
        title(b,&mut c,"Add a little atmosphere","Effects are applied in order, from the base up.");
        let columns=((c.w/b.px(190.0)).floor() as usize).clamp(1,4);
        let gap=b.px(14.0);let w=(c.w-gap*(columns-1) as f32)/columns as f32;
        for category in ["weather","camera","color","motion","stylize","light"] {
            if editor.category.as_deref().is_some_and(|chosen|chosen!=category){continue;}
            let effects:Vec<_>=editor.effects.iter().filter(|e|e.category()==category).collect();
            if effects.is_empty(){continue;}
            c.y+=b.px(14.0);
            text(b,Rect::new(c.x,c.y,c.w,b.px(26.0)),&capitalize(category),16.0,600,b.p.ink1,Align::Start);c.y+=b.px(34.0);
            for chunk in effects.chunks(columns){
                for (col,effect) in chunk.iter().enumerate(){
                    let r=Rect::new(c.x+col as f32*(w+gap),c.y,w,w*0.62+b.px(65.0));
                    let pic=Rect::new(r.left,r.top,w,w*0.62);
                    let id=key(("builder-effect",&effect.id));
                    controls::card(b,r,b.hovered(id));
                    controls::card_picture(b,r,pic,|b,pic|{
                        if let Some(image)=app.images.borrow_mut().get(&effect.dir.join("preview.jpg"),(pic.width() as u32,pic.height() as u32)){crate::stage::image_round(b,pic,&image,1.0,0.0);}
                        else{b.fill(pic,0.0,b.p.well);b.icon(pic,glyph::WALLPAPERS,28.0,b.p.ink3);}
                    });
                    text(b,Rect::new(r.left+b.px(12.0),pic.bottom+b.px(8.0),w-b.px(24.0),b.px(24.0)),&effect.name,13.0,600,b.p.ink1,Align::Start);
                    text(b,Rect::new(r.left+b.px(12.0),pic.bottom+b.px(34.0),w-b.px(24.0),b.px(19.0)),&format!("{} · {:.1} ms",if effect.animated{"Moving"}else{"Still"},effect.cost),11.0,400,b.p.ink3,Align::Start);
                    b.hit(id,r,Role::Button,&format!("Add {}",effect.name),action(Command::AddEffect(effect.id.clone())),Behavior::Click);
                    b.focus_ring(id,r,b.px(24.0));
                }
                c.y+=w*0.62+b.px(80.0);
            }
        }
        if editor.effects.is_empty(){notice(b,&mut c,"No effect packages found. Add a custom WGSL effect to get started.");}
        c.y+b.px(20.0)+offset
    });
}

fn shader(app:&App,b:&mut B,area:Rect) {
    let editor=active(app).unwrap();
    let r=Rect::new(area.left+b.px(8.0),area.top+b.px(12.0),area.width()-b.px(16.0),area.height()-b.px(24.0));
    b.fill(r,b.px(20.0),b.p.panel);
    let filename=editor.shader.as_ref().and_then(|p|p.file_name()).map(|p|p.to_string_lossy()).unwrap_or_default();
    text(b,Rect::new(r.left+b.px(20.0),r.top+b.px(12.0),r.width()-b.px(40.0),b.px(30.0)),&filename,13.0,600,b.p.ink1,Align::Start);
    b.hline(r.left+b.px(16.0),r.right-b.px(16.0),r.top+b.px(52.0),b.p.line);
    let code=Rect::new(r.left+b.px(58.0),r.top+b.px(69.0),r.width()-b.px(78.0),r.height()-b.px(92.0));
    let mono=TextStyle::new("Cascadia Mono",b.px(12.0));
    let row=b.px(22.0);let max=(code.height()/row) as usize;
    for (i,source) in editor.source.lines().take(max).enumerate(){
        let y=code.top+i as f32*row;
        let error=editor.diagnostics.iter().any(|e|e.line==i+1);
        let number=Rect::new(r.left+b.px(12.0),y,b.px(32.0),row);
        b.text_in(number,&(i+1).to_string(),&mono,if error{b.p.ink1}else{b.p.ink3},Align::End);
        if error{b.fill(Rect::new(r.left+b.px(5.0),y+b.px(6.0),b.px(3.0),b.px(10.0)),b.px(1.5),b.p.ink1);}
        if app.field.as_ref().is_none_or(|f|f.id!=FieldId::ShaderCode){
            b.text_in(Rect::new(code.left,y,code.width(),row),source,&mono,if source.trim().starts_with("//"){b.p.ink3}else{b.p.ink1},Align::Start);
            if error {b.hline(code.left,code.right,y+row-b.px(2.0),b.p.ink2);}
        }
    }
    let k=crate::app::field_key(&FieldId::ShaderCode);
    b.hit(k,code,Role::TextInput,"Shader source",Action::Field(FieldId::ShaderCode),Behavior::Click);
    if let Some(field)=app.field.as_ref().filter(|f|f.id==FieldId::ShaderCode){
        field.field.set_rect(code);field.field.set_style(&mono,b.p.ink1);
        b.list.ops.push(mypaper_ui::Op::TextField{rect:code,field:field.field.clone(),revision:field.field.revision()});
    }
}

fn capitalize(value:&str)->String {let mut chars=value.chars();chars.next().map(|c|c.to_uppercase().collect::<String>()+chars.as_str()).unwrap_or_default()}
