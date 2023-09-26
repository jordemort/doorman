use anyhow::{anyhow, Context, Result};
use handlebars::Handlebars;
use rust_embed::RustEmbed;
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::Path;
use yore::code_pages::CP437;

#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/templates/dos"]
struct Asset;

pub struct Templates<'a> {
    hbars: Handlebars<'a>,
}

impl Templates<'_> {
    pub fn new() -> Templates<'static> {
        let mut hbars = Handlebars::new();
        hbars.register_escape_fn(handlebars::no_escape);

        return Templates { hbars };
    }

    pub fn render_string<T: Serialize>(&self, template: &String, vars: T) -> Result<String> {
        return Ok(self.hbars.render_template(template.as_str(), &vars)?);
    }

    pub fn render_template<T: Serialize>(&self, name: &str, vars: T) -> Result<String> {
        if let Some(asset) = Asset::get(format!("{0}.hbr", name).as_str()) {
            let template = String::from_utf8(asset.data.to_vec())
                .with_context(|| format!("While converting template {} to UTF-8", name))?;

            return Ok(self
                .render_string(&template, &vars)
                .with_context(|| format!("While rendirng template {}", name))?);
        }

        return Err(anyhow!("Couldn't find template for {0}", name));
    }

    pub fn write_dos<T: Serialize>(&self, name: &str, dir: &Path, vars: T) -> Result<()> {
        let rendered = self.render_template(name, vars)?;
        let crlf = rendered.replace("\n", "\r\n");
        let encoded = CP437.encode_lossy(&crlf, 63);
        let path = dir.join(name.to_uppercase());

        let mut output = fs::File::create(&path)?;
        output.write_all(&encoded)?;

        return Ok(());
    }
}
