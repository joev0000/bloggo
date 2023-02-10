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
pub mod helper;
pub mod value;

use chrono::{DateTime, NaiveDate, Utc};
use error::Error;
use handlebars::Handlebars;
use helper::FormatDateTimeHelper;
use log::{debug, info};
use pulldown_cmark::{html, Parser};
use std::{
    borrow::Borrow,
    collections::BTreeMap,
    fs::File,
    io::{BufRead, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};
use value::Value;

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
        {
            let mut out_path = PathBuf::new();
            out_path.push(&self.dest_dir);
            out_path.push("index.html");
            let out = File::create(out_path)?;
            self.generate_index(&posts, out)?;
        }
        self.generate_tag_indexes(&posts)?;

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
    fn generate_index<W>(&self, posts: &[Post], out: W) -> Result<()>
    where
        W: Write,
    {
        let p: Vec<Value> = posts.iter()
            .map(|e| Value::Map(e.clone()))
            .collect();
        let mut data = BTreeMap::new();
        data.insert("posts", Value::Array(p));
        self.handlebars.render_to_write("index", &data, out)?;
        Ok(())
    }

    fn generate_tag_indexes(&self, posts: &Vec<Post>) -> Result<()> {
        // generate index structure
        let mut tag_index: BTreeMap<String, Vec<Post>> = BTreeMap::new();
        for post in posts {
            match post.get("tags") {
                Some(Value::String(s)) => {
                    crate::insert_to_vec(&mut tag_index, s, post.clone());
                }
                Some(Value::Array(a)) => {
                    for element in a {
                        if let Some(s) = element.as_string() {
                            crate::insert_to_vec(&mut tag_index, &s, post.clone());
                        }
                    }
                }
                _ => {}
            }
        }

        // iterate through the tags
        for (k, v) in tag_index.iter() {
            let posts = v.iter().map(|e| Value::Map(e.clone())).collect();

            let mut out_path = PathBuf::new();
            out_path.push(&self.dest_dir);
            out_path.push(k);
            fs::create_dir_all(&out_path)?;

            out_path.push("index.html");
            let out = File::create(out_path)?;

            let mut data = BTreeMap::new();
            data.insert("tag", Value::String(k.clone()));
            data.insert("posts", Value::Array(posts));

            self.handlebars
                .render_to_write("index", &data, out)?;
        }
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

/// Insert a value into a vector in a BTreeMap. Create a new Vec if necessary.
fn insert_to_vec<K, V>(map: &mut BTreeMap<K, Vec<V>>, key: &K, value: V)
where
    K: Ord + Clone,
{
    if let Some(v) = map.get_mut(key) {
        v.push(value);
    } else {
        map.insert(key.clone(), vec![value]);
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
