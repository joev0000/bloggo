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
use serde::{ser::SerializeMap, Serialize, Serializer};
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
    pub fn new(src_dir: String, dest_dir: String, base_url: String) -> Self {
        let mut handlebars = Handlebars::new();
        handlebars.register_helper("formatDateTime", Box::new(FormatDateTimeHelper::new()));
        Self {
            src_dir,
            dest_dir,
            base_url,
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
        let all_posts = self.parse_posts()?;

        // Generate tag indices.
        let tag_index = self.generate_tag_indexes(&all_posts);
        let tags: Vec<&String> = tag_index.keys().collect();
        debug!("Tags: {:?}", tags);

        let all_posts_refs: Vec<&Post> = all_posts.iter().collect();
        let mut render_context = RenderContext {
            tag: None,
            tags: &tags,
            posts: &all_posts_refs,
        };
        self.render_index(&render_context, &PathBuf::from("index.html"))?;
        self.render_atom_feed(&all_posts_refs, &PathBuf::from("atom.xml"))?;
        for (tag, posts) in &tag_index {
            let mut index_path = PathBuf::from(tag);
            index_path.push("index.html");
            render_context.tag = Some(tag);
            render_context.posts = posts;
            self.render_index(&render_context, &index_path)?;

            let mut feed_path = PathBuf::from(tag);
            feed_path.push("atom.xml");
            self.render_atom_feed(posts, &feed_path)?;
        }
        self.render_posts(&all_posts)?;
        Ok(())
    }

    fn render_index(&self, render_context: &RenderContext, path: &Path) -> Result<()> {
        let mut p = PathBuf::new();
        p.push(&self.dest_dir);
        p.push(path);
        info!("Rendering index to {}", p.display());
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = BufWriter::new(File::create(p)?);
        self.generate_index(render_context, &mut out)?;
        out.flush()?;
        Ok(())
    }

    fn render_atom_feed(&self, posts: &[&Post], path: &Path) -> Result<()> {
        let mut p = PathBuf::new();
        p.push(&self.dest_dir);
        p.push(path);
        info!("Rendering feed to {}", p.display());
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = BufWriter::new(File::create(p)?);
        atom::generate_atom_feed(posts, &mut out)?;
        out.flush()?;
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
    fn generate_index<W>(&self, render_context: &RenderContext, out: &mut W) -> Result<()>
    where
        W: Write,
    {
        self.handlebars
            .render_to_write("index", render_context, out)?;
        Ok(())
    }

    fn generate_tag_indexes<'b>(&'b self, posts: &'b Vec<Post>) -> BTreeMap<String, Vec<&'b Post>> {
        let mut tag_index: BTreeMap<String, Vec<&Post>> = BTreeMap::new();

        let mut add_post_to_index = |s: &String, p| {
            if let Some(v) = tag_index.get_mut(s) {
                v.push(p);
            } else {
                let v = vec![p];
                tag_index.insert(s.clone(), v);
            }
        };

        // generate index structure
        for post in posts {
            match post.get("tags") {
                Some(Value::String(s)) => {
                    add_post_to_index(s, post);
                }
                Some(Value::Array(a)) => {
                    for element in a {
                        if let Some(s) = element.as_string() {
                            add_post_to_index(&s, post)
                        }
                    }
                }
                _ => {}
            }
        }
        tag_index
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

/// Structure to hold the data values rendered by Handlebars
struct RenderContext<'a> {
    tag: Option<&'a str>,
    tags: &'a Vec<&'a String>,
    posts: &'a Vec<&'a Post>,
}

impl<'a> Serialize for RenderContext<'a> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let len: usize = 2 + usize::from(self.tag.is_some());
        let mut s = serializer.serialize_map(Some(len))?;
        self.tag.map(|t| s.serialize_entry("tag", t));
        s.serialize_entry("tags", self.tags)?;
        s.serialize_entry("posts", self.posts)?;
        s.end()
    }
}
