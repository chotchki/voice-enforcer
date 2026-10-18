use std::sync::LazyLock;

use regex::Regex;
use tree_sitter::{Language, Node, Parser};

use super::{Annotation, ProseSpan, SpanBuilder};
use crate::types::{Level, Register};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Swift,
    TypeScript,
    Tsx,
    JavaScript,
    Python,
    Bash,
}

impl Lang {
    pub fn from_extension(ext: &str) -> Option<Self> {
        Some(match ext {
            "rs" => Lang::Rust,
            "swift" => Lang::Swift,
            "ts" | "mts" | "cts" => Lang::TypeScript,
            "tsx" => Lang::Tsx,
            "js" | "jsx" | "mjs" | "cjs" => Lang::JavaScript,
            "py" => Lang::Python,
            "sh" | "bash" | "zsh" => Lang::Bash,
            _ => return None,
        })
    }

    fn language(self) -> Language {
        match self {
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
            Lang::Swift => tree_sitter_swift::LANGUAGE.into(),
            Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
            Lang::Bash => tree_sitter_bash::LANGUAGE.into(),
        }
    }

    fn is_comment(kind: &str) -> bool {
        matches!(
            kind,
            "comment" | "line_comment" | "block_comment" | "multiline_comment"
        )
    }

    /// Declarations a comment directly above documents. Variable-like kinds only count at top
    /// level or in a type body, so a comment above a local `const` stays a line comment.
    fn is_declaration(self, node: Node) -> bool {
        let kind = node.kind();
        let parent = node.parent().map_or("", |p| p.kind());
        match self {
            Lang::Rust => matches!(
                kind,
                "function_item"
                    | "function_signature_item"
                    | "struct_item"
                    | "enum_item"
                    | "union_item"
                    | "trait_item"
                    | "impl_item"
                    | "type_item"
                    | "mod_item"
                    | "const_item"
                    | "static_item"
                    | "macro_definition"
                    | "field_declaration"
                    | "enum_variant"
                    | "associated_type"
                    | "foreign_mod_item"
            ),
            Lang::Swift => {
                matches!(
                    kind,
                    "function_declaration"
                        | "class_declaration"
                        | "protocol_declaration"
                        | "typealias_declaration"
                        | "init_declaration"
                        | "deinit_declaration"
                        | "subscript_declaration"
                        | "enum_entry"
                        | "protocol_function_declaration"
                        | "protocol_property_declaration"
                        | "associatedtype_declaration"
                        | "operator_declaration"
                ) || (kind == "property_declaration"
                    && matches!(
                        parent,
                        "source_file" | "class_body" | "protocol_body" | "enum_class_body"
                    ))
            }
            Lang::TypeScript | Lang::Tsx | Lang::JavaScript => {
                matches!(
                    kind,
                    "function_declaration"
                        | "generator_function_declaration"
                        | "class_declaration"
                        | "abstract_class_declaration"
                        | "method_definition"
                        | "method_signature"
                        | "abstract_method_signature"
                        | "function_signature"
                        | "interface_declaration"
                        | "type_alias_declaration"
                        | "enum_declaration"
                        | "public_field_definition"
                        | "field_definition"
                        | "property_signature"
                        | "export_statement"
                        | "module"
                        | "internal_module"
                        | "ambient_declaration"
                ) || (matches!(kind, "lexical_declaration" | "variable_declaration")
                    && matches!(parent, "program" | "export_statement"))
            }
            Lang::Python => matches!(
                kind,
                "function_definition" | "class_definition" | "decorated_definition"
            ),
            Lang::Bash => kind == "function_definition",
        }
    }

    fn is_attribute(self, kind: &str) -> bool {
        self == Lang::Rust && kind == "attribute_item"
    }
}

static BUILTIN_DIRECTIVES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?:eslint-|@ts-|swiftlint:|noqa\b|type:\s*ignore|shellcheck\s|prettier-ignore|biome-ignore|rustfmt:|#region|#endregion|pylint:|fmt:\s*(?:on|off|skip)\b|NOLINT|istanbul ignore|c8 ignore|clang-format (?:on|off))",
    )
    .expect("valid directive regex")
});

static BACKTICK_CODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"`[^`\n]+`").expect("valid backtick regex"));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
    Plain,
    OuterDoc,
    InnerDoc,
}

/// One comment node, already stripped of its markers.
struct Piece<'t> {
    node: Node<'t>,
    /// `(0-based row, text)` per line.
    lines: Vec<(usize, String)>,
    block: bool,
    style: Style,
    own_line: bool,
    shebang: bool,
}

impl Piece<'_> {
    fn first_row(&self) -> usize {
        self.lines[0].0
    }

    fn last_row(&self) -> usize {
        self.lines[self.lines.len() - 1].0
    }

    /// The row the comment syntax ends on, which can sit past the last prose line (` */`).
    fn end_row(&self) -> usize {
        if self.block {
            self.node.end_position().row
        } else {
            self.last_row()
        }
    }
}

pub(super) fn extract(lang: Lang, src: &str, directives: &[Regex]) -> Vec<ProseSpan> {
    let mut parser = Parser::new();
    parser
        .set_language(&lang.language())
        .expect("bundled grammars match the tree-sitter ABI");
    let tree = parser
        .parse(src, None)
        .expect("parsing without a timeout or cancellation flag always yields a tree");
    let root = tree.root_node();

    let mut pieces = Vec::new();
    let mut docstrings = Vec::new();
    collect(lang, root, src, &mut pieces, &mut docstrings);
    pieces.retain(|p| !is_skipped(p, directives));

    let mut blocks: Vec<Vec<Piece>> = Vec::new();
    for piece in pieces {
        match blocks.last_mut() {
            Some(block) if merges(block.last().expect("blocks are never empty"), &piece, src) => {
                block.push(piece);
            }
            _ => blocks.push(vec![piece]),
        }
    }

    let first_code = first_code_byte(root);
    let mut out: Vec<ProseSpan> = blocks
        .iter()
        .filter(|b| !is_license(b))
        .filter_map(|b| {
            let level = classify(lang, b, first_code);
            build(b.iter().flat_map(|p| p.lines.iter()), level)
        })
        .collect();
    out.extend(
        docstrings
            .into_iter()
            .filter_map(|(lines, level)| build(lines.iter(), level)),
    );
    out.sort_by_key(ProseSpan::first_line);
    out
}

fn collect<'t>(
    lang: Lang,
    node: Node<'t>,
    src: &str,
    pieces: &mut Vec<Piece<'t>>,
    docstrings: &mut Vec<(Vec<(usize, String)>, Level)>,
) {
    if Lang::is_comment(node.kind()) {
        pieces.push(piece(lang, node, src));
        return;
    }
    if lang == Lang::Python
        && let Some((string, level)) = python_docstring(node)
    {
        docstrings.push((docstring_lines(string, src), level));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect(lang, child, src, pieces, docstrings);
    }
}

fn piece<'t>(lang: Lang, node: Node<'t>, src: &str) -> Piece<'t> {
    let raw = &src[node.byte_range()];
    let row = node.start_position().row;
    let line_start = node.start_byte() - node.start_position().column;
    let own_line = src[line_start..node.start_byte()].trim().is_empty();

    let (style, block, lines) = if let Some(body) = raw.strip_prefix("/*") {
        let style = match body.chars().next() {
            Some('!') if lang == Lang::Rust => Style::InnerDoc,
            Some('*') if !body.starts_with("*/") => Style::OuterDoc,
            _ => Style::Plain,
        };
        let body = body.strip_suffix("*/").unwrap_or(body);
        let body = if style == Style::Plain {
            body
        } else {
            &body[1..]
        };
        let lines = body
            .split('\n')
            .enumerate()
            .map(|(i, l)| {
                let l = l.trim();
                let l = l.strip_prefix('*').filter(|_| i > 0).unwrap_or(l);
                (row + i, l.trim().to_owned())
            })
            .collect::<Vec<_>>();
        (style, true, trim_blank_ends(lines))
    } else {
        let text = raw.trim_end();
        let (style, rest) = if let Some(rest) = text.strip_prefix("//!") {
            (Style::InnerDoc, rest)
        } else if let Some(rest) = text.strip_prefix("///").filter(|r| !r.starts_with('/')) {
            (Style::OuterDoc, rest)
        } else if let Some(rest) = text.strip_prefix("//") {
            (Style::Plain, rest)
        } else {
            (Style::Plain, text.trim_start_matches('#'))
        };
        (style, false, vec![(row, rest.trim().to_owned())])
    };
    Piece {
        node,
        lines,
        block,
        style,
        own_line,
        shebang: row == 0 && raw.starts_with("#!"),
    }
}

/// Drops blank first and last lines of a block comment (`/*` alone on its line), keeping one.
fn trim_blank_ends(mut lines: Vec<(usize, String)>) -> Vec<(usize, String)> {
    while lines.len() > 1 && lines[lines.len() - 1].1.is_empty() {
        lines.pop();
    }
    let lead = lines.iter().take_while(|(_, l)| l.is_empty()).count();
    lines.drain(..lead.min(lines.len() - 1));
    lines
}

fn is_skipped(piece: &Piece, directives: &[Regex]) -> bool {
    let first = &piece.lines[0].1;
    piece.shebang
        || BUILTIN_DIRECTIVES.is_match(first)
        || directives.iter().any(|d| d.is_match(first))
}

fn merges(prev: &Piece, next: &Piece, src: &str) -> bool {
    !prev.block
        && !next.block
        && prev.own_line
        && next.own_line
        && prev.style == next.style
        && next.first_row() == prev.last_row() + 1
        && src[prev.node.end_byte()..next.node.start_byte()]
            .trim()
            .is_empty()
}

fn is_license(block: &[Piece]) -> bool {
    let first = block[0].lines.iter().find(|(_, l)| !l.is_empty());
    first.is_some_and(|(_, l)| {
        let l = l.to_ascii_lowercase();
        l.starts_with("copyright") || l.starts_with("spdx-license-identifier")
    })
}

fn first_code_byte(root: Node) -> usize {
    let mut cursor = root.walk();
    root.named_children(&mut cursor)
        .find(|n| !Lang::is_comment(n.kind()) && n.kind() != "hash_bang_line")
        .map_or(usize::MAX, |n| n.start_byte())
}

/// Inner docs are module level; an own-line block directly above a declaration (attributes
/// between are fine) is item level; anything else before all code is module level.
fn classify(lang: Lang, block: &[Piece], first_code: usize) -> Level {
    let first = &block[0];
    let last = &block[block.len() - 1];
    if first.style == Style::InnerDoc {
        return Level::Module;
    }
    if first.own_line {
        let mut end_row = last.end_row();
        let mut next = last.node.next_named_sibling();
        while let Some(n) = next {
            if n.start_position().row != end_row + 1 {
                break;
            }
            if lang.is_attribute(n.kind()) {
                end_row = n.end_position().row;
                next = n.next_named_sibling();
                continue;
            }
            if lang.is_declaration(n) {
                return Level::Item;
            }
            break;
        }
    }
    if first.node.start_byte() < first_code
        && first.node.parent().is_some_and(|p| p.parent().is_none())
    {
        return Level::Module;
    }
    Level::Line
}

/// A string that is the first statement of a module, class or function body.
fn python_docstring(node: Node) -> Option<(Node, Level)> {
    let (body, level) = match node.kind() {
        "module" => (node, Level::Module),
        "function_definition" | "class_definition" => {
            (node.child_by_field_name("body")?, Level::Item)
        }
        _ => return None,
    };
    let mut cursor = body.walk();
    let first = body
        .named_children(&mut cursor)
        .find(|n| n.kind() != "comment")?;
    if first.kind() != "expression_statement" || first.named_child_count() != 1 {
        return None;
    }
    let string = first.named_child(0)?;
    (string.kind() == "string").then_some((string, level))
}

fn docstring_lines(string: Node, src: &str) -> Vec<(usize, String)> {
    let raw = src[string.byte_range()].trim_start_matches(|c: char| c.is_ascii_alphabetic());
    let quote = ["\"\"\"", "'''", "\"", "'"]
        .into_iter()
        .find(|q| raw.starts_with(q))
        .unwrap_or("");
    let body = raw
        .strip_prefix(quote)
        .and_then(|b| b.strip_suffix(quote))
        .unwrap_or(raw);
    let row = string.start_position().row;
    let lines: Vec<&str> = body.split('\n').collect();
    let indent = lines
        .iter()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    let lines = lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let l = if i == 0 {
                l
            } else {
                l.get(indent..).unwrap_or(l)
            };
            (row + i, l.trim_end().to_owned())
        })
        .collect();
    trim_blank_ends(lines)
}

fn build<'a>(lines: impl Iterator<Item = &'a (usize, String)>, level: Level) -> Option<ProseSpan> {
    let mut b = SpanBuilder::new(Register::Comment, Some(level));
    for (row, text) in lines {
        let line = row + 1;
        if b.len() > 0 {
            b.push("\n", line);
        }
        let mut last = 0;
        for m in BACKTICK_CODE.find_iter(text) {
            b.push_prose(&text[last..m.start()], line);
            let range = b.push_mask(line);
            b.annotate(Annotation::InlineCode {
                range,
                content: text[m.start() + 1..m.end() - 1].to_owned(),
            });
            last = m.end();
        }
        b.push_prose(&text[last..], line);
    }
    b.finish()
}

#[cfg(test)]
mod tests;
