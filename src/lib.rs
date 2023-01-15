//! # Bloggo
//!
//! A static site generator library that merges Markdown posts into HTML
//! templates, with the output being the HTML and CSS that are deployable
//! as a static web site.
//!
//! # Examples
//!
//! ```no_run
//! use bloggo::Builder;
//!
//! let mut bloggo = Builder::new()
//!     .src_dir("src")
//!     .dest_dir("dest")
//!     .build();
//!
//! bloggo.clean();
//! bloggo.build();
//! ```

pub mod error;
pub mod fs;

use error::Error;
use handlebars::Handlebars;
use log::{debug, info};
use pulldown_cmark::{html, Parser};
use serde::ser::Serialize;
use std::fs::File;
use std::io::BufRead;
use std::path::{Path, PathBuf};

/// A Result type whose [Err] contains a Bloggo [Error].
pub type Result<T> = std::result::Result<T, Error>;

/// An instance of Bloggo that contains configuration settings and stateful
/// context for rendering posts.
///
/// Use [Builder] to create a new instance.
///
/// # Examples
///
/// ```no_run
/// use bloggo::Builder;
///
/// let mut bloggo = Builder::new()
///     .src_dir("source")
///     .dest_dir("destination")
///     .build();
///
/// bloggo.clean();
/// bloggo.build();
/// ```
pub struct Bloggo<'a> {
    src_dir: String,
    dest_dir: String,
    handlebars: Handlebars<'a>,
}

impl<'a> Bloggo<'a> {
    /// Create a new Bloggo instance with the given source and destination
    /// directories.
    pub fn new(src_dir: impl Into<String>, dest_dir: impl Into<String>) -> Self {
        Self {
            src_dir: src_dir.into(),
            dest_dir: dest_dir.into(),
            handlebars: Handlebars::new(),
        }
    }

    /// Removes the destination directory.
    pub fn clean(&self) -> Result<()> {
        info!("Cleaning build directory: {}", self.dest_dir);
        fs::remove_dir_all(&self.dest_dir).map_err(|e| e.into())
    }

    /// Builds the static site by copying assets and generating HTML.
    pub fn build(&mut self) -> Result<()> {
        info!("Building from {} to {}", self.src_dir, self.dest_dir);
        // make dest dir.
        debug!("Creating build directory: {}", self.dest_dir);
        fs::create_dir_all(&self.dest_dir)?;
        self.copy_assets()?;
        let _posts = self.render_posts()?;
        // self.generate_index(posts)?;
        Ok(())
    }

    fn copy_assets(&self) -> Result<usize> {
        fn is_hidden(path: &Path) -> bool {
            path.file_name()
                .and_then(|os| os.to_str())
                .map(|s| s.starts_with('.'))
                .unwrap_or(false)
        }

        let mut count = 0_usize;

        let mut src_dir = PathBuf::new();
        src_dir.push(&self.src_dir);
        src_dir.push("assets");

        for rde in fs::recursive_read_dir(&src_dir)? {
            let de = rde?;
            let src_path = de.path();
            if !is_hidden(&src_path) {
                let mut dest_path = PathBuf::new();
                dest_path.push(&self.dest_dir);
                dest_path.push(src_path.strip_prefix(&src_dir)?);

                if src_path.is_dir() {
                    info!("Creating directory {}", dest_path.display());
                    fs::create_dir_all(dest_path)?;
                } else {
                    info!("Copying {} to {}", src_path.display(), dest_path.display());
                    std::fs::copy(src_path, dest_path)?;
                    count += 1;
                }
            }
        }
        Ok(count)
    }

    fn render_posts(&mut self) -> Result<usize> {
        let mut render_count = 0_usize;
        let mut src_dir = PathBuf::new();
        src_dir.push(&self.src_dir);
        src_dir.push("posts");

        for rde in fs::recursive_read_dir(&src_dir)? {
            let de = rde?;
            let src_path = de.path();
            if src_path.extension().and_then(|s| s.to_str()) == Some("md") {
                let mut dest_path = PathBuf::new();
                dest_path.push(&self.dest_dir);
                dest_path.push(src_path.strip_prefix(&src_dir)?);
                dest_path.set_extension("html");
                info!(
                    "Rendering {} to {}",
                    src_path.display(),
                    dest_path.display()
                );
                let src_vec = std::fs::read(&src_path)?;
                let mut src: &[u8] = src_vec.as_ref();
                let mut line = String::with_capacity(256);
                let mut len = src.read_line(&mut line)?;
                if len == 0 {
                    return Err(Error::UnexpectedEOF(src_path.into_os_string()));
                }

                if line.starts_with("---") {
                    let mut buf = String::with_capacity(1024);
                    loop {
                        line.clear();
                        len = src.read_line(&mut line)?;
                        if len == 0 {
                            return Err(Error::UnexpectedEOF(src_path.into_os_string()));
                        }
                        if line.starts_with("---") {
                            break;
                        }
                        buf.push_str(&line);
                    }
                    let front_matter = Bloggo::parse_yaml_data(&buf)?;
                    debug!("front matter: {:?}", front_matter);

                    let text = Bloggo::parse_text(src)?;

                    if let serde_yaml::Value::Mapping(data) = front_matter {
                        let template_key: serde_yaml::Value = "layout".into();
                        let template = data
                            .get(template_key)
                            .and_then(serde_yaml::Value::as_str)
                            .unwrap_or("default");

                        let text_key: serde_yaml::Value = "text".into();
                        let text_value: serde_yaml::Value = text.into();
                        // TODO: There's probably a better way to do this
                        // without cloning.
                        let mut data = data.clone();
                        data.insert(text_key, text_value);

                        self.render_post(template, data, dest_path)?;
                    } else {
                        return Err(Error::Other(
                            "Unexpected YAML type found in front matter.".into(),
                        ));
                    }
                }
            }
            render_count += 1;
        }

        Ok(render_count)
    }

    fn render_post(
        &mut self,
        template: &str,
        data: impl Serialize,
        dest_file: impl AsRef<Path>,
    ) -> Result<()> {
        self.register_template(template)?;
        let out = File::create(dest_file)?;
        self.handlebars.render_to_write(template, &data, out)?;
        Ok(())
    }

    fn register_template(&mut self, name: &str) -> Result<()> {
        if !self.handlebars.has_template(name) {
            let mut file_name = String::from(name);
            file_name.push_str(".html.hbs");

            let mut path = PathBuf::new();
            path.push(&self.src_dir);
            path.push("templates");
            path.push(file_name);

            debug!("Registering template {} at {}", name, path.display());

            self.handlebars
                .register_template_file(name, path)
                .map_err(Error::from)
        } else {
            debug!("Template {} already registered.", name);
            Ok(())
        }
    }

    fn parse_text(mut src: &[u8]) -> Result<String> {
        let mut line = String::with_capacity(256);
        let mut md = String::with_capacity(16 * 1024);
        let mut len = src.read_line(&mut line)?;
        while len != 0 {
            md.push_str(&line);
            line.clear();
            len = src.read_line(&mut line)?;
        }
        let mut text = String::with_capacity(16 * 1024);
        let parser = Parser::new(&md);
        html::push_html(&mut text, parser);
        Ok(text)
    }

    fn parse_yaml_data(yaml: &str) -> Result<serde_yaml::Value> {
        serde_yaml::from_str::<serde_yaml::value::Value>(yaml)
            .map_err(|e| Error::Other(format!("YAML deserialization failure: {}", e)))
    }

    fn _parse_toml_data(toml: &str) -> Result<impl Serialize> {
        toml::de::from_str::<toml::value::Value>(toml)
            .map_err(|e| Error::Other(format!("TOML serialization failure: {}", e)))
    }
}

/// A builder for Bloggo instances.
///
/// # Examples
///
/// ```
/// use bloggo::Builder;
///
/// let bloggo = Builder::new()
///     .src_dir("source")
///     .dest_dir("destination")
///     .build();
/// ```
pub struct Builder {
    src_dir: String,
    dest_dir: String,
}

impl Builder {
    /// Create a new Builder with the default source and destination
    /// directories.
    pub fn new() -> Self {
        Self {
            src_dir: String::from("src/"),
            dest_dir: String::from("dest/"),
        }
    }

    /// Set the source directory for Bloggo.
    pub fn src_dir(mut self, src_dir: impl Into<String>) -> Self {
        self.src_dir = src_dir.into();
        self
    }

    /// Set the destination directory for Bloggo.
    pub fn dest_dir(mut self, dest_dir: impl Into<String>) -> Self {
        self.dest_dir = dest_dir.into();
        self
    }

    /// Build a Bloggo struct with the previously configured values.
    pub fn build<'a>(self) -> Bloggo<'a> {
        Bloggo::new(self.src_dir, self.dest_dir)
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}
