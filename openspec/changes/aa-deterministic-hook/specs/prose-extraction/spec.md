# Spec Delta

## Purpose

Which files yield prose for the rules, which register that prose is in (long-form or comment) and which parts of a file are never treated as prose.

## ADDED Requirements

### Requirement: File types

voice-enforcer SHALL pick an extractor by file extension: `.md` and `.markdown` as long-form markdown; `.txt` as long-form plain text; `.rs`, `.swift`, `.ts`, `.tsx`, `.js`, `.jsx`, `.mjs`, `.cjs`, `.py`, `.sh`, `.bash` and `.zsh` as code whose comments are prose. Any other extension SHALL yield no prose, so every rule passes it.

#### Scenario: Markdown is long-form
- **WHEN** a `.md` file is evaluated
- **THEN** its prose is in the long-form register

#### Scenario: Unknown extension yields nothing
- **WHEN** a `.csv` file is evaluated
- **THEN** no rule produces a finding

### Requirement: Markdown prose

In markdown, headings, paragraphs, list items, block quotes and table cells SHALL be long-form prose. Fenced and indented code blocks, HTML blocks, YAML or TOML front matter and link destinations SHALL NOT be prose. Inline code SHALL be masked so phrase, regex, Oxford comma and sentence rules don't see its contents, while the file-link rule still does. Link text SHALL be prose.

#### Scenario: Code block is ignored
- **WHEN** a banned phrase appears only inside a fenced code block
- **THEN** no finding is reported

#### Scenario: Inline code is masked
- **WHEN** a banned phrase appears only inside backticks
- **THEN** no phrase finding is reported

#### Scenario: Heading is prose
- **WHEN** a banned phrase appears in a `##` heading
- **THEN** a finding is reported on the heading's line

### Requirement: Code comments are prose

In code files every comment SHALL be prose in the comment register, and nothing else in the file SHALL be. Comment markers (`//`, `///`, `//!`, `#`, `/*`, `*/` and a leading `*` on block-comment lines) SHALL be stripped before rules see the text.

#### Scenario: String literals are not prose
- **WHEN** a banned phrase appears in a Rust string literal and nowhere else
- **THEN** no finding is reported

### Requirement: Comment blocks

Line comments of the same style (a `///` run and a `//` run stay separate), each on its own line and on consecutive lines, SHALL merge into one comment block.

#### Scenario: Consecutive line comments merge
- **WHEN** three `//` lines sit on consecutive lines with no code between them
- **THEN** the rules see one comment block spanning the three lines

### Requirement: Docstrings and code inside comments

A Python docstring (a string literal that is the first statement of a module, class or function) SHALL count as a comment. Backtick-quoted code inside a comment SHALL be masked the same way as markdown inline code.

#### Scenario: Docstring is a comment
- **WHEN** a Python function's first statement is a triple-quoted string
- **THEN** that string is prose in the comment register

#### Scenario: Backtick code in a comment is masked
- **WHEN** a comment holds a banned phrase only inside backticks
- **THEN** no phrase finding is reported

### Requirement: Comment levels

Every comment block SHALL be classified at exactly one level, checked in this order:
- **module**: Rust inner doc comments (`//!`, `/*! */`) and a Python module docstring
- **item**: an own-line block ending on the line directly above a declaration (an attribute or decorator between them is allowed), and Python class and function docstrings
- **module**: any other comment block that precedes all code in the file
- **line**: every other comment block

#### Scenario: Rust doc comment is item level
- **WHEN** a `///` block sits directly above a `fn`
- **THEN** it is classified item

#### Scenario: Blank line breaks attachment
- **WHEN** a `//` block is separated from the following `fn` by a blank line and is not the file's first comment block
- **THEN** it is classified line

#### Scenario: File header is module level
- **WHEN** a TypeScript file opens with a comment block before any import or declaration
- **THEN** it is classified module

#### Scenario: Header attached to a declaration is item level
- **WHEN** a file opens with a comment block on the line directly above a function
- **THEN** it is classified item

### Requirement: What counts as a declaration

Declarations SHALL be functions, methods, classes, structs, enums and their variants, fields, traits, protocols, interfaces, type aliases, impls, extensions, modules and constants. A variable declaration SHALL only count at the top level of a file or in a type body, so a comment above a local variable stays a line comment.

#### Scenario: Comment above a local variable is line level
- **WHEN** a comment sits directly above a `const` inside a TypeScript function body
- **THEN** it is classified line

### Requirement: Comments that are never prose

The following SHALL NOT be prose: a shebang line, a comment whose text starts with `SPDX-License-Identifier` or `Copyright`, and a tool directive comment matching a built-in pattern (`eslint-`, `@ts-`, `swiftlint:`, `noqa`, `type: ignore`, `shellcheck`, `prettier-ignore`, `biome-ignore`, `rustfmt:`, `#region`, `#endregion`) or a pattern from the ruleset's `comment_directives`. A file whose first five lines contain `@generated` or `DO NOT EDIT` SHALL yield no prose at all.

#### Scenario: Directive is skipped
- **WHEN** a comment reads `eslint-disable-next-line no-console`
- **THEN** it is not prose

#### Scenario: Generated file is skipped
- **WHEN** line 1 of a `.rs` file reads `// @generated by build.rs`
- **THEN** no rule produces a finding for that file

### Requirement: Findings map to real lines

Every piece of extracted prose SHALL keep its line numbers in the evaluated text, so a finding reports the line a person would open in an editor.

#### Scenario: Finding inside a merged block
- **WHEN** a banned phrase sits on the third line of a comment block that starts on line 20
- **THEN** the finding reports line 22
