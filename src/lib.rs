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

pub mod atom;
pub mod error;
pub mod fs;

use chrono::{DateTime, NaiveDate, Utc};
use error::Error;
use handlebars::{Context, Handlebars, Helper, HelperDef, RenderContext, RenderError, ScopedJson};
use log::{debug, info};
use pulldown_cmark::{html, Parser};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use std::{
    borrow::Borrow,
    collections::BTreeMap,
    fs::File,
    io::{BufRead, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

/// A Result type whose [Err] contains a Bloggo [Error].
pub type Result<T> = std::result::Result<T, Error>;

/// A Post is a mapping of [String]s to [Value]s.
type Post = BTreeMap<String, Value>;

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
    base_url: String,
    handlebars: Handlebars<'a>,
}

impl<'a> Bloggo<'a> {
    /// Create a new Bloggo instance with the given source and destination
    /// directories.
    pub fn new(
        src_dir: impl Into<String>,
        dest_dir: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Self {
        let mut handlebars = Handlebars::new();
        handlebars.register_helper("formatDateTime", Box::new(FormatDateTimeHelper::new()));
        Self {
            src_dir: src_dir.into(),
            dest_dir: dest_dir.into(),
            base_url: base_url.into(),
            handlebars,
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

        let mut template_dir = PathBuf::new();
        template_dir.push(&self.src_dir);
        template_dir.push("templates");
        info!(
            "Registering templates in directory {}",
            template_dir.display()
        );
        self.handlebars
            .register_templates_directory(".html.hbs", template_dir)?;

        fs::create_dir_all(&self.dest_dir)?;
        self.copy_assets()?;
        let posts = self.parse_posts()?;
        self.render_posts(&posts)?;
        self.generate_index(&posts)?;

        {
            let mut feed_path = PathBuf::new();
            feed_path.push(&self.dest_dir);
            feed_path.push("atom.xml");
            let mut feed = BufWriter::new(File::create(feed_path)?);

            atom::generate_atom_feed(&posts, &mut feed)?;
            feed.flush()?;
        }
        Ok(())
    }

    /// Copy all files from the "assets/" source directory to the
    /// destination directory.
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

    /// Render the posts in the source directory to the destination directory.
    fn render_posts(&self, posts: &Vec<Post>) -> Result<()> {
        for post in posts {
            self.render_post(post)?;
        }

        Ok(())
    }

    /// Render an individual post to the destination directory.
    fn render_post(&self, post: &Post) -> Result<()> {
        let template = post
            .get("layout")
            .and_then(|v| v.as_string())
            .unwrap_or_else(|| String::from("default"));
        if let Some(Value::String(filename)) = post.get("path") {
            let mut pathbuf = PathBuf::new();
            pathbuf.push(&self.dest_dir);
            pathbuf.push(filename);
            pathbuf.set_extension("html");
            let out = File::create(&pathbuf)?;
            info!("Rendering post to {}", pathbuf.display());
            self.handlebars.render_to_write(&template, &post, out)?;
        }
        Ok(())
    }

    /// Generate an index page using the index template and the list of posts.
    fn generate_index(&self, posts: &Vec<Post>) -> Result<()> {
        let mut out_path = PathBuf::new();
        out_path.push(&self.dest_dir);
        out_path.push("index.html");
        let out = File::create(out_path)?;
        self.handlebars.render_to_write("index", &posts, out)?;
        Ok(())
    }

    /// Parse the posts in the source directory.
    fn parse_posts(&self) -> Result<Vec<Post>> {
        let mut posts = Vec::new();
        let mut src_dir = PathBuf::new();
        src_dir.push(&self.src_dir);
        src_dir.push("posts");

        for rde in fs::recursive_read_dir(&src_dir)? {
            let de = rde?;
            let src_path = de.path();
            posts.push(self.parse_post(src_path)?);
        }
        posts.sort_by_cached_key(|p| {
            p.get("date")
                .and_then(|v| v.as_string())
                .and_then(|s| DateTime::parse_from_str(&s, "%+").ok())
                .unwrap_or_else(|| DateTime::parse_from_str("1970-01-01T00:00:00Z", "%+").unwrap())
            // TODO: Make the Unix Epoch a constant so it doesn't need to be
            // parsed each time.
        });
        posts.reverse();
        Ok(posts)
    }

    /// Parse a post from the given [Path].
    fn parse_post<P>(&self, path: P) -> Result<Post>
    where
        P: AsRef<Path>,
    {
        // open a file
        // read first line
        let p = path.as_ref();
        debug!("parse_post: Parsing {}", p.display());
        let file = File::open(p)?;
        let mut line = String::with_capacity(256);
        let mut buf = BufReader::new(file);
        if buf.read_line(&mut line)? == 0 {
            return Err(Error::UnexpectedEOF(p.as_os_str().to_os_string()));
        }
        let mut post = if line.starts_with("---") {
            debug!("parse_post: Parsing YAML front matter.");
            let front_matter = read_until(&mut buf, "---")?;
            if let Value::Map(map) = parse_yaml_data(front_matter.as_str())? {
                Ok(map)
            } else {
                Err(Error::Other("Parsed YAML is not a mapping.".to_string()))
            }
        } else {
            Err(Error::Other("Missing front matter.".to_string()))
        }?;
        let mut rest_of_file = String::new();
        buf.read_to_string(&mut rest_of_file)?;

        let mut text = String::with_capacity(rest_of_file.len());
        if p.extension().and_then(|s| s.to_str()) == Some("md") {
            let parser = Parser::new(&rest_of_file);
            html::push_html(&mut text, parser);
        } else {
            text = rest_of_file;
        }
        post.insert("text".into(), text.into());

        let mut dest_path_buf = p
            .strip_prefix(&self.src_dir)?
            .strip_prefix("posts")?
            .to_path_buf();
        dest_path_buf.set_extension("html");

        let cows = dest_path_buf.to_string_lossy();
        let filename: &str = cows.borrow();
        post.insert("path".into(), filename.into());

        let mut url = String::from(&self.base_url);
        url.push('/');
        url.push_str(filename);
        post.insert("url".into(), url.into());
        if !post.contains_key("date") {
            if let Some(date) = extract_date_from_str(filename) {
                let formatted = format!("{}", date.format("%+"));
                post.insert("date".into(), formatted.into());
            }
        }
        Ok(post)
    }
}

/// Attempt to extract a date from the first ten characters of a string.
/// The date will have a time of midnight, UTC
fn extract_date_from_str(s: &str) -> Option<DateTime<Utc>> {
    let mut truncated = String::from(s);
    truncated.truncate(10);
    NaiveDate::parse_from_str(truncated.as_str(), "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| DateTime::from_utc(dt, Utc))
}

/// Parse a YAML [str] into a [Value].
fn parse_yaml_data(yaml: &str) -> Result<Value> {
    let yval = serde_yaml::from_str::<serde_yaml::value::Value>(yaml)
        .map_err(|e| Error::Other(format!("YAML deserialization failure: {}", e)))?;
    yval.try_into()
}

/// Read a [BufRead] into a [String] until a linke with the given prefix
///
///# Example
///
/// ```compile_fail
/// use std::io::BufReader;
/// use bloggo::read_until;
///
/// let mut bufread = BufReader::new("Line One\nLine Two\n-----\nLine Three".as_bytes());
///
/// let two_lines = read_until(&mut bufread, "---");
/// assert_eq!("Line One\nLine Two\n", two_lines.unwrap());
/// ```
fn read_until<B>(buf_read: &mut B, prefix: &str) -> Result<String>
where
    B: std::io::BufRead,
{
    let mut s = String::new();
    let mut line = String::new();

    loop {
        let bytes_read = buf_read.read_line(&mut line)?;
        if bytes_read == 0 {
            return Err(Error::Other("Unexpected end of file.".to_string()));
        } else if line.starts_with(prefix) {
            return Ok(s);
        } else {
            s.push_str(line.as_str());
        }
        line.clear();
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
    base_url: String,
}

impl Builder {
    /// Create a new Builder with the default source and destination
    /// directories.
    pub fn new() -> Self {
        Self {
            src_dir: String::from("src/"),
            dest_dir: String::from("dest/"),
            base_url: String::from(""),
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

    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Build a Bloggo struct with the previously configured values.
    pub fn build<'a>(self) -> Bloggo<'a> {
        Bloggo::new(self.src_dir, self.dest_dir, self.base_url)
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

/// An unsigned number, either an integer or floating point number.
#[derive(Debug)]
pub enum Number {
    Integer(i64),
    Float(f64),
}

/// A value parsed from post front matter. This enum is necessary since
/// each front matter type (YAML, TOML, etc.) is different.
#[derive(Debug)]
pub enum Value {
    Null,
    Boolean(bool),
    Number(Number),
    String(String),
    Array(Vec<Value>),
    Map(BTreeMap<String, Value>),
}

impl Value {
    /// Return [Some]([String]) if the Value is a string, [None] otherwise.
    ///
    /// # Example
    ///
    /// ```
    /// use bloggo::Value;
    ///
    /// let string  = Value::String("a string".to_string());
    /// let boolean = Value::Boolean(true);
    ///
    /// assert_eq!(Some("a string".to_string()), string.as_string());
    /// assert_eq!(None, boolean.as_string());
    /// ```
    pub fn as_string(&self) -> Option<String> {
        match self {
            Value::String(s) => Some(s.clone()),
            _ => None,
        }
    }
}

impl From<String> for Value {
    fn from(s: String) -> Value {
        Value::String(s)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Value {
        Value::String(String::from(s))
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Value {
        Value::Boolean(b)
    }
}

impl From<i64> for Value {
    fn from(i: i64) -> Value {
        Value::Number(Number::Integer(i))
    }
}

impl From<f64> for Value {
    fn from(f: f64) -> Value {
        Value::Number(Number::Float(f))
    }
}

impl From<Vec<Value>> for Value {
    fn from(v: Vec<Value>) -> Value {
        Value::Array(v)
    }
}

impl From<BTreeMap<String, Value>> for Value {
    fn from(m: BTreeMap<String, Value>) -> Value {
        Value::Map(m)
    }
}

impl TryFrom<serde_yaml::Value> for Value {
    type Error = Error;

    fn try_from(yval: serde_yaml::Value) -> Result<Value> {
        match yval {
            serde_yaml::Value::Null => Ok(Value::Null),
            serde_yaml::Value::Bool(b) => Ok(Value::Boolean(b)),
            serde_yaml::Value::Number(n) => {
                if let Some(f) = n.as_f64() {
                    Ok(Value::Number(Number::Float(f)))
                } else if let Some(i) = n.as_i64() {
                    Ok(Value::Number(Number::Integer(i)))
                } else {
                    Err(Error::Other(format!(
                        "Unknown number format while parsing YAML: {}",
                        n
                    )))
                }
            }
            serde_yaml::Value::String(s) => Ok(Value::String(s)),
            serde_yaml::Value::Sequence(s) => {
                let mut vec = Vec::with_capacity(s.len());
                for yv in s {
                    let bv: Value = yv.try_into()?;
                    vec.push(bv);
                }
                Ok(Value::Array(vec))
            }
            serde_yaml::Value::Mapping(m) => {
                let mut map = BTreeMap::new();
                for (k, v) in m.iter() {
                    if let Some(key) = k.as_str() {
                        let value: Value = v.to_owned().try_into()?;
                        map.insert(String::from(key), value);
                    }
                }
                Ok(Value::Map(map))
            }
            serde_yaml::Value::Tagged(tv) => {
                let v: Value = tv.value.try_into()?;
                Ok(v)
            }
        }
    }
}

impl Serialize for Value {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Value::String(s) => serializer.serialize_str(s),
            Value::Boolean(b) => serializer.serialize_bool(*b),
            Value::Number(Number::Integer(i)) => serializer.serialize_i64(*i),
            Value::Number(Number::Float(f)) => serializer.serialize_f64(*f),
            Value::Array(v) => {
                let mut s = serializer.serialize_seq(Some(v.len()))?;
                for e in v {
                    s.serialize_element(e)?;
                }
                s.end()
            }
            Value::Map(m) => {
                let mut s = serializer.serialize_map(Some(m.len()))?;
                for (k, v) in m.iter() {
                    s.serialize_entry(k, v)?;
                }
                s.end()
            }
            Value::Null => serializer.serialize_none(),
        }
    }
}

/// A Handlebars helper that formats date string properties for rendering.
///
/// The first parameter is the property to be formatted. It must be a String
/// that contains a date in the ISO8601 format.
/// The second parameter is optional, and specifies the [chrono::format::strftime] format
/// specification.
/// If no format is specified, `%c` is used as a default.
///
/// # Examples
/// ```no_compile
/// // date: "2023-02-04T15:38:42Z"
///
/// {{#if date}}
///   {{formatDateTime date "%A, %B %e, %Y at %l:%M%P"}}
/// {{/if}}
///
/// // output: "Saturday, February 4, 2023 at 3:38pm"
/// ```
struct FormatDateTimeHelper {}

impl FormatDateTimeHelper {
    /// Create a new FormatDateTimeHelper.
    fn new() -> Self {
        Self {}
    }
}

impl HelperDef for FormatDateTimeHelper {
    fn call_inner<'reg: 'rc, 'rc>(
        &self,
        h: &Helper<'reg, 'rc>,
        _: &'reg Handlebars<'reg>,
        _: &'rc Context,
        _: &mut RenderContext<'reg, 'rc>,
    ) -> std::result::Result<ScopedJson<'reg, 'rc>, RenderError> {
        let value = h
            .param(0)
            .map(|pj| pj.value())
            .filter(|v| !v.is_null())
            .and_then(|v| v.as_str())
            .ok_or_else(|| RenderError::new("Property cannot be converted to string."))?;

        let format = h
            .param(1)
            .map(|p| p.value())
            .filter(|v| !v.is_null())
            .and_then(|v| v.as_str())
            .unwrap_or("%c");

        let dt = DateTime::parse_from_str(value, "%+").map_err(|e| {
            RenderError::new(format!("Could not parse as datetime: {} ({})", value, e))
        })?;

        Ok(ScopedJson::Derived(serde_json::value::Value::String(
            format!("{}", dt.format(format)),
        )))
    }
}
