//! Doc sections
//! Arguments plus Returns bullets with ordered documentation.

/// Split doc lines into summary plus section bodies.
pub(crate) struct SectionSplit {
    pub(crate) summary: Vec<String>,
    pub(crate) arguments: Vec<String>,
    pub(crate) returns: Vec<String>,
    pub(crate) has_arguments: bool,
    pub(crate) has_returns: bool,
}

/// Split doc lines into summary plus section bodies.
pub(crate) fn split_sections(
    class_name: &str,
    method_name: &str,
    lines: &[String],
    span: &syn::Ident,
) -> Result<SectionSplit, syn::Error> {
    let mut summary = Vec::new();
    let mut arguments = Vec::new();
    let mut returns = Vec::new();
    let mut seen_arguments = false;
    let mut seen_returns = false;
    let mut current = 0u8;
    for line in lines {
        if line == "# Arguments" {
            if seen_arguments {
                return Err(syn::Error::new_spanned(
                    span,
                    format!("{class_name}.{method_name}: keep a single Arguments section"),
                ));
            }
            seen_arguments = true;
            current = 1;
            continue;
        }
        if line == "# Returns" {
            if seen_returns {
                return Err(syn::Error::new_spanned(
                    span,
                    format!("{class_name}.{method_name}: keep a single Returns section"),
                ));
            }
            seen_returns = true;
            current = 2;
            continue;
        }
        match current {
            1 => arguments.push(line.clone()),
            2 => returns.push(line.clone()),
            _ => summary.push(line.clone()),
        }
    }
    Ok(SectionSplit {
        summary,
        arguments,
        returns,
        has_arguments: seen_arguments,
        has_returns: seen_returns,
    })
}

/// Parse one Arguments bullet into its name plus description.
pub(crate) fn parse_argument_bullet(
    class_name: &str,
    method_name: &str,
    line: &str,
    span: &syn::Ident,
) -> Result<(String, String), syn::Error> {
    let malformed = || {
        syn::Error::new_spanned(
            span,
            format!(
                "{class_name}.{method_name}: write Arguments bullets as '- [name] description'"
            ),
        )
    };
    if !line.starts_with("- [") {
        return Err(malformed());
    }
    let Some(close) = line.find(']') else {
        return Err(malformed());
    };
    if close <= 3 {
        return Err(malformed());
    }
    let name = line[3..close].to_owned();
    if name.is_empty() {
        return Err(malformed());
    }
    let rest = &line[close + 1..];
    if !rest.starts_with(' ') || rest.trim().is_empty() {
        return Err(malformed());
    }
    Ok((name, rest.trim().to_owned()))
}

/// Parse one Returns bullet into its description.
pub(crate) fn parse_return_bullet(
    class_name: &str,
    method_name: &str,
    line: &str,
    span: &syn::Ident,
) -> Result<String, syn::Error> {
    if !line.starts_with("- ") || line[2..].trim().is_empty() {
        return Err(syn::Error::new_spanned(
            span,
            format!("{class_name}.{method_name}: write Returns bullets as '- description'"),
        ));
    }
    Ok(line[2..].trim().to_owned())
}

/// Document method params from Arguments bullets in param order.
pub(crate) fn document_params(
    class_name: &str,
    method_name: &str,
    lines: &[String],
    has_section: bool,
    labels: &[String],
    span: &syn::Ident,
) -> Result<Vec<String>, syn::Error> {
    if labels.is_empty() {
        if has_section {
            return Err(syn::Error::new_spanned(
                span,
                format!(
                    "{class_name}.{method_name}: remove the Arguments section for a method with no params"
                ),
            ));
        }
        return Ok(Vec::new());
    }
    if !has_section {
        return Err(syn::Error::new_spanned(
            span,
            format!(
                "{class_name}.{method_name}: add an Arguments section with bullets for each param"
            ),
        ));
    }
    let mut bullets: Vec<(String, String)> = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        bullets.push(parse_argument_bullet(class_name, method_name, line, span)?);
    }
    for (name, _) in &bullets {
        if bullets.iter().filter(|entry| &entry.0 == name).count() > 1 {
            return Err(syn::Error::new_spanned(
                span,
                format!("{class_name}.{method_name}: keep a single Arguments bullet per param"),
            ));
        }
    }
    for label in labels {
        if !bullets.iter().any(|entry| &entry.0 == label) {
            return Err(syn::Error::new_spanned(
                span,
                format!("{class_name}.{method_name}: add an Arguments bullet for param '{label}'"),
            ));
        }
    }
    for (name, _) in &bullets {
        if !labels.contains(name) {
            return Err(syn::Error::new_spanned(
                span,
                format!(
                    "{class_name}.{method_name}: remove Arguments bullet for unknown param '{name}'"
                ),
            ));
        }
    }
    let mut ordered = Vec::new();
    for label in labels {
        for (name, description) in &bullets {
            if name == label {
                ordered.push(description.clone());
            }
        }
    }
    Ok(ordered)
}

/// Document method returns from Returns bullets in order.
pub(crate) fn document_returns(
    class_name: &str,
    method_name: &str,
    lines: &[String],
    has_section: bool,
    arity: usize,
    span: &syn::Ident,
) -> Result<Vec<String>, syn::Error> {
    if arity == 0 {
        if has_section {
            return Err(syn::Error::new_spanned(
                span,
                format!(
                    "{class_name}.{method_name}: remove the Returns section for a method with no returns"
                ),
            ));
        }
        return Ok(Vec::new());
    }
    if !has_section {
        return Err(syn::Error::new_spanned(
            span,
            format!("{class_name}.{method_name}: add Returns bullets for each return"),
        ));
    }
    let mut bullets = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        bullets.push(parse_return_bullet(class_name, method_name, line, span)?);
    }
    if bullets.len() != arity {
        if arity == 1 {
            return Err(syn::Error::new_spanned(
                span,
                format!("{class_name}.{method_name}: write one Returns bullet for one return"),
            ));
        }
        return Err(syn::Error::new_spanned(
            span,
            format!(
                "{class_name}.{method_name}: write {arity} Returns bullets for {arity} returns"
            ),
        ));
    }
    Ok(bullets)
}
