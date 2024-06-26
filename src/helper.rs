use chrono::DateTime;
use handlebars::{
    Context, Handlebars, Helper, HelperDef, RenderContext, RenderError, RenderErrorReason,
    ScopedJson,
};

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
        h: &Helper<'rc>,
        _: &'reg Handlebars<'reg>,
        _: &'rc Context,
        _: &mut RenderContext<'reg, 'rc>,
    ) -> std::result::Result<ScopedJson<'rc>, RenderError> {
        let value = h
            .param(0)
            .map(|pj| pj.value())
            .filter(|v| !v.is_null())
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                RenderErrorReason::Other("Property cannot be converted to string.".into())
            })?;

        let format = h
            .param(1)
            .map(|p| p.value())
            .filter(|v| !v.is_null())
            .and_then(|v| v.as_str())
            .unwrap_or("%c");

        let dt = DateTime::parse_from_str(value, "%+").map_err(|e| {
            RenderErrorReason::Other(format!("Could not parse as datetime: {} ({})", value, e))
        })?;

        Ok(ScopedJson::Derived(serde_json::value::Value::String(
            format!("{}", dt.format(format)),
        )))
    }
}

/// A Handlebars helper that joins string arrays.
///
/// The first parameter is the property to be formatted. It must be an
/// array of Strings.
/// The second parameter is optional, and specifies the join seperator.
/// If no format is specified, ", " is used as a default.
///
/// # Examples
/// ```no_compile
/// // tags: ["alpha", "beta"]
///
/// {{join tags " + "}}
///
/// // output: "alpha + beta"
/// ```
pub(crate) struct JoinHelper {}

impl JoinHelper {
    /// Create a new JoinHelper.
    pub fn new() -> Self {
        Self {}
    }
}

impl HelperDef for JoinHelper {
    fn call_inner<'reg: 'rc, 'rc>(
        &self,
        h: &Helper<'rc>,
        _: &'reg Handlebars<'reg>,
        _: &'rc Context,
        _: &mut RenderContext<'reg, 'rc>,
    ) -> std::result::Result<ScopedJson<'rc>, RenderError> {
        let value: Vec<&str> = h
            .param(0)
            .map(|pj| pj.value())
            .filter(|v| !v.is_null())
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|e| e.as_str()).collect())
            .ok_or_else(|| {
                RenderErrorReason::Other("Property cannot be converted to array.".into())
            })?;

        let sep = h
            .param(1)
            .map(|p| p.value())
            .filter(|v| !v.is_null())
            .and_then(|v| v.as_str())
            .unwrap_or(", ");

        Ok(ScopedJson::Derived(serde_json::value::Value::String(
            value.join(sep),
        )))
    }
}
