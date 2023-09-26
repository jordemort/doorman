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
      return Templates { hbars: hbars };
  }

  pub fn render_string<T: Serialize>(&self, template: &String, vars: T) -> Result<String, String> {
      return match self.hbars.render_template(template.as_str(), &vars) {
          Ok(rendered) => Ok(rendered),
          Err(e) => return Err(format!("Couldn't render template: {0}", e)),
      };
  }

  pub fn render_template<T: Serialize>(&self, name: &str, vars: T) -> Result<String, String> {
      let template = match Asset::get(format!("{0}.hbr", name).as_str()) {
          Some(asset) => match String::from_utf8(asset.data.to_vec()) {
              Ok(data) => data,
              Err(e) => return Err(format!("Couldn't load template for {0}: {1}", name, e)),
          },
          None => return Err(format!("Couldn't find template for {0}", name)),
      };
      return self.render_string(&template, vars);
  }

  pub fn write_dos<T: Serialize>(&self, name: &str, dir: &Path, vars: T) -> Result<(), String> {
      let rendered = match self.render_template(name, vars) {
          Ok(rendered) => rendered,
          Err(e) => return Err(e),
      };

      let crlf = rendered.replace("\n", "\r\n");
      let encoded = CP437.encode_lossy(&crlf, 63);
      let path = dir.join(name.to_uppercase());

      let mut output = match fs::File::create(&path) {
          Ok(file) => file,
          Err(e) => return Err(format!("Couldn't create {0}: {1}", path.display(), e)),
      };

      match output.write_all(&encoded) {
          Ok(_) => return Ok(()),
          Err(e) => {
              return Err(format!(
                  "Couldn't write data to {0}: {1}",
                  path.display(),
                  e
              ))
          }
      }
  }
}
