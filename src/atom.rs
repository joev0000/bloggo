use crate::{Post, Result};
use std::io::Write;

pub(crate) fn generate_atom_feed<W>(posts: &[&Post], out: &mut W) -> Result<()>
where
    W: Write,
{
    // write intro
    writeln!(
        out,
        r##"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">"##,
    )?;

    for post in posts {
        writeln!(out, "  <entry>")?;
        if let Some(t) = post.get("title").and_then(|v| v.as_string()) {
            writeln!(out, "    <title>{}</title>", t)?;
        }
        if let Some(dt) = post.get("date").and_then(|v| v.as_string()) {
            writeln!(out, "    <published>{}</published>", dt)?;
        }
        if let Some(l) = post.get("url").and_then(|v| v.as_string()) {
            writeln!(out, r#"    <link href="{}" />"#, l)?;
        }
        writeln!(out, "  </entry>")?;
    }

    // write outro
    writeln!(out, "</feed>")?;
    Ok(())
}
