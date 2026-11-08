//! Package a composition without ever writing into an existing theme directory.
use super::*;

pub fn publish(app: &App, root: &Path) -> Result<PathBuf> {
    let editor = active(app).context("no composition is open")?;
    let mut value = raw(app);
    let name = value["name"].as_str().unwrap_or("").trim().to_owned();
    if name.is_empty() {
        bail!("give your wallpaper a name");
    }
    value["hidden"] = json!(false);
    for layer in value["wallpaper"]["layers"]
        .as_array_mut()
        .context("missing layers")?
    {
        if let Some(effect) = editor
            .effects
            .iter()
            .find(|e| layer["effect"].as_str() == Some(&e.id))
        {
            strip_defaults(layer.get_mut("params"), &effect.schemas());
        }
    }
    if value["wallpaper"]["base"]["kind"] == "shader" {
        if let Some(source) = value["wallpaper"]["base"]["source"].as_str()
            && let Ok(code) = std::fs::read_to_string(editor.dir.join(source))
        {
            strip_defaults(
                value["wallpaper"]["base"].get_mut("params"),
                &shader_params(&code),
            );
        }
    }
    let text = serde_json::to_string_pretty(&value)?;
    validate_theme(&text)?;
    let slug = slug(&name);
    std::fs::create_dir_all(root)?;
    // Creating the directory is the reservation. Even a malformed theme with this name is
    // left untouched, including when another Studio saves at the same time.
    let target = (1..)
        .find_map(|suffix| {
            let dir = root.join(if suffix == 1 {
                slug.clone()
            } else {
                format!("{slug}-{suffix}")
            });
            match std::fs::create_dir(&dir) {
                Ok(()) => Some(Ok(dir)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(e) => Some(Err(e)),
            }
        })
        .expect("a suffix is available")?;
    let written = (|| -> Result<()> {
        copy_tree(&editor.dir, &target)?;
        let scratch = target.join(".editor");
        if scratch.is_dir() {
            std::fs::remove_dir_all(scratch)?;
        }
        let mut credits: Vec<Value> = std::fs::read(target.join("credits.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        for layer in value["wallpaper"]["layers"].as_array().unwrap() {
            if let Some(effect) = editor
                .effects
                .iter()
                .find(|e| layer["effect"].as_str() == Some(&e.id))
            {
                let dest = target.join("effects").join(&effect.id);
                if !dest.exists() {
                    copy_tree(&effect.dir, &dest)?;
                }
                credits.push(json!({"effect":effect.id,"name":effect.name,"author":effect.author,"license":effect.license,"url":effect.url}));
            }
        }
        let base = &value["wallpaper"]["base"];
        if base["kind"] == "glsl" {
            credits.push(json!({"file":base["source"],"author":base["author"],"license":base["license"],"url":base["url"]}));
        }
        crate::docs::write_atomic(
            &target.join("credits.json"),
            &serde_json::to_vec_pretty(&credits)?,
        )?;
        // The engine writes preview.jpg atomically into the draft. Copying that complete
        // image packages the same pixels the canvas showed, without capturing the desktop.
        crate::docs::write_atomic(&target.join("theme.json"), text.as_bytes())?;
        mypaper_theme::LoadedTheme::load(&target)?;
        Ok(())
    })();
    if let Err(error) = written {
        // Only this function's successfully reserved child can reach this cleanup.
        let _ = std::fs::remove_dir_all(&target);
        return Err(error);
    }
    Ok(target)
}

fn strip_defaults(params: Option<&mut Value>, schemas: &[Value]) {
    if let Some(params) = params.and_then(Value::as_object_mut) {
        params.retain(|id, value| {
            !schemas.iter().any(|schema| {
                schema["id"].as_str() == Some(id.as_str()) && same_param(&schema["default"], value)
            })
        });
    }
}

fn slug(name: &str) -> String {
    let slug = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "wallpaper".into()
    } else {
        slug.chars().take(72).collect()
    }
}
