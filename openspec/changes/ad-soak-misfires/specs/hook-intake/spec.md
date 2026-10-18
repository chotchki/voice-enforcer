# Spec Delta

## MODIFIED Requirements

### Requirement: Only changed lines are judged

The hook SHALL report a finding only when the finding's lines intersect a changed line: one holding a word that is added or changed relative to the current on-disk file. A change to whitespace alone (a re-wrapped paragraph, re-indentation) SHALL change no line. When the file does not exist yet every line counts as changed. In a git repository, a finding inside a prose block whose source text already appears in a file tracked at HEAD, ignoring whitespace differences, SHALL NOT be reported.

#### Scenario: Pre-existing violation on an untouched line
- **WHEN** a file already contains a banned phrase on line 3 and an Edit changes only line 10
- **THEN** no finding is reported for line 3

#### Scenario: Touching a line makes its violations count
- **WHEN** an Edit rewrites line 3 and the banned phrase survives the rewrite
- **THEN** the finding for line 3 is reported

#### Scenario: New file is judged whole
- **WHEN** a Write creates a file that does not exist on disk, outside any git repository
- **THEN** findings on every line are reported

#### Scenario: Re-wrapped paragraph
- **WHEN** an Edit only moves the line breaks of a paragraph that holds a banned phrase
- **THEN** no finding is reported for that paragraph

#### Scenario: Text moved into a new file
- **WHEN** a Write creates a new file in a git repository and one of its comments, re-indented, matches a comment in a file tracked at HEAD
- **THEN** no finding is reported inside that comment, and the new file's other comments are judged as usual

#### Scenario: Reworded comment moved to a new file
- **WHEN** a comment moves to a new file and one word in it changes on the way
- **THEN** its findings are reported, since that text isn't at HEAD

#### Scenario: Git unavailable
- **WHEN** `git` isn't installed or fails while a write in a git repository has findings
- **THEN** the findings are reported as if the HEAD check didn't exist
