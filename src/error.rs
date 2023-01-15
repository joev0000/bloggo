//! Error type for Bloggo
//!
//! Provides a single error type that wraps other errors that may be
//! encountered by Bloggo during its processing.

use std::ffi::OsString;
use std::fmt::{self, Formatter};
use std::io;

/// The Error type for Bloggo.
#[derive(Debug)]
pub enum Error {
    /// The error was caused by an underlying [io::Error]
    IoError(io::Error),

    /// Handlebars could not parse the template.
    TemplateError(Box<handlebars::TemplateError>),

    /// Handlebars encountered an error while rending a post.
    RenderError(Box<handlebars::RenderError>),

    /// There was an unexpected End of File while parsing front matter.
    UnexpectedEOF(OsString),

    /// Some other unspecfied error described in the message.
    Other(String),
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::IoError(e) => Some(e),
            Error::TemplateError(e) => Some(e),
            Error::RenderError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<&Error> for String {
    /// Converts this error reference into a [String].
    fn from(error: &Error) -> String {
        match error {
            Error::IoError(ioe) => format!("{}", ioe),
            Error::TemplateError(te) => format!("{}", te),
            Error::RenderError(re) => format!("{}", re),
            Error::UnexpectedEOF(s) => format!("Unexpected end of file: {}", s.to_string_lossy()),
            Error::Other(s) => s.to_string(),
        }
    }
}

impl From<Error> for String {
    /// Converts this [Error] into a [String].
    fn from(error: Error) -> String {
        String::from(&error)
    }
}

impl From<io::Error> for Error {
    /// Converts a [std::io::Error] into a wrapped Bloggo [Error]
    fn from(error: io::Error) -> Self {
        Error::IoError(error)
    }
}

impl From<handlebars::TemplateError> for Error {
    /// Converts a [handlebars::TemplateError] into a wrapped Bloggo [Error]
    fn from(error: handlebars::TemplateError) -> Self {
        Error::TemplateError(Box::new(error))
    }
}

impl From<handlebars::RenderError> for Error {
    /// Converts a [handlebars::RenderError] into a wrapped Bloggo [Error]
    fn from(error: handlebars::RenderError) -> Self {
        Error::RenderError(Box::new(error))
    }
}

impl From<std::path::StripPrefixError> for Error {
    /// Converts a [std::path::StripPrefixError] into a wrapped Bloggo [Error]
    fn from(error: std::path::StripPrefixError) -> Self {
        Error::Other(error.to_string())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(&String::from(self))
    }
}
