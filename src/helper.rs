use chrono::DateTime;
use handlebars::{Context, Handlebars, Helper, HelperDef, RenderContext, RenderError, ScopedJson};

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
pub(crate) struct FormatDateTimeHelper {}

impl FormatDateTimeHelper {
    /// Create a new FormatDateTimeHelper.
    pub fn new() -> Self {
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
