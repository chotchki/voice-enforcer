# Spec Delta

## MODIFIED Requirements

### Requirement: Oxford comma rule

An `oxford_comma` rule SHALL report a finding for a comma placed directly before the final `and` or `or` of a list, where a list is at least two earlier comma-separated items in the same sentence, each at most `max_item_words` words long (default 4). A comma before `and` or `or` with no list before it in the sentence SHALL NOT be a finding, and neither SHALL one after a comma-separated part starting with the word "not", which is a contrast rather than an item. The finding's matched text SHALL be the list from its first item through the conjunction, so two findings in one file never match the same text unless they quote the same list.

#### Scenario: Serial comma flagged
- **WHEN** the prose reads "red, green, and blue"
- **THEN** one finding is reported on the comma before "and", matching "red, green, and"

#### Scenario: No serial comma
- **WHEN** the prose reads "red, green and blue"
- **THEN** no finding is reported

#### Scenario: Two independent clauses
- **WHEN** the prose reads "I fixed the bug, and the build passes now."
- **THEN** no finding is reported

#### Scenario: Contrast aside
- **WHEN** the prose reads "It reads the saved copy, not the live one, and the cache stays cold."
- **THEN** no finding is reported

#### Scenario: Two lists in one file
- **WHEN** a file holds "Pick tea, coffee, or water." and later "Bring forks, plates, and cups."
- **THEN** the two findings have different matched text and different dispute ids

### Requirement: File link findings

A mention that resolves SHALL be a finding at the rule's severity, with a suggestion holding the relative link from the markdown file to the resolved file. A path mention that does not resolve SHALL be a `warn` finding saying the named file wasn't found when its parent directory exists beside the markdown file or at the repository root, and SHALL produce no finding otherwise. A bare name that does not resolve SHALL produce no finding. A path or inline-code mention that matches more than one file SHALL be a `warn` finding saying how many files it matches, whatever the rule's severity. A mention that names an existing directory SHALL produce no finding. A relative link whose target does not exist SHALL be a `warn` finding.

#### Scenario: Unlinked existing file
- **WHEN** `docs/guide.md` mentions `src/main.rs` in backticks, not as a link, and `src/main.rs` exists at the repository root
- **THEN** a finding at the rule's severity suggests `[src/main.rs](../src/main.rs)`

#### Scenario: Linked file is fine
- **WHEN** the prose holds `[src/main.rs](src/main.rs)` and the file exists
- **THEN** no finding is reported

#### Scenario: Named file is missing
- **WHEN** the prose mentions `src/gone.rs`, `src/` exists at the repository root and `src/gone.rs` does not
- **THEN** a `warn` finding says the file wasn't found

#### Scenario: Path outside the repository
- **WHEN** the prose mentions `disc/levels/one.bin` and no `disc/` directory exists beside the markdown file or at the repository root
- **THEN** no finding is reported

#### Scenario: Bare name that isn't found
- **WHEN** the prose mentions `manifest.json` in backticks and no file by that name exists
- **THEN** no finding is reported

#### Scenario: Name matches several files
- **WHEN** the prose mentions `lib.rs` in backticks and the repository holds `a/src/lib.rs` and `b/src/lib.rs`
- **THEN** a `warn` finding says the name matches 2 files, and never that it wasn't found

#### Scenario: Directory mention
- **WHEN** the prose mentions `tools/sdk-1.4/` or `out/Viewer.app`, and that path is an existing directory
- **THEN** no finding is reported

#### Scenario: Broken relative link
- **WHEN** the prose holds `[setup](docs/setup.md)` and that file does not exist
- **THEN** a `warn` finding is reported
