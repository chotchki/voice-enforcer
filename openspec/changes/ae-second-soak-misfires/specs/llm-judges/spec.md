# Spec Delta

## MODIFIED Requirements

### Requirement: Findings only on changed lines

A judge finding SHALL be reported only when the text it quotes holds a word the write added or changed, or a point where the write deleted text. A change elsewhere on the quote's lines SHALL NOT count. When the quote can't be placed in the file's text, it SHALL count if one of its lines holds a deletion or a changed word the quote also holds. A `diff` judge's finding SHALL also be dropped inside text that already exists at HEAD, as the hook drops deterministic findings; a `whole` judge's SHALL NOT, since a passage copied from elsewhere in the repository is what a whole judge exists to catch.

#### Scenario: Whole judge quotes old text
- **WHEN** a whole judge quotes a sentence on a line the write didn't change
- **THEN** that finding is dropped

#### Scenario: Old sentence in a changed paragraph
- **WHEN** an Edit changes one word in the last sentence of a paragraph and a diff judge quotes the paragraph's first sentence
- **THEN** that finding is dropped

#### Scenario: One-line paragraph gains a common word
- **WHEN** a paragraph is written on one line, an Edit inserts "the" into its last sentence and a whole judge quotes its first sentence, which also holds "the"
- **THEN** that finding is dropped

#### Scenario: One-line paragraph loses a word
- **WHEN** a paragraph is written on one line, an Edit deletes a word from its last sentence and a judge quotes its first sentence
- **THEN** that finding is dropped

#### Scenario: Deletion inside the quote
- **WHEN** an Edit deletes a word from the middle of a sentence and a judge quotes that sentence
- **THEN** the finding is reported

#### Scenario: Quote across a link
- **WHEN** an Edit changes the text of a markdown link mid-sentence and a judge quotes that sentence, which holds the link text but not the link's target
- **THEN** the finding is reported

#### Scenario: Copied paragraph
- **WHEN** a Write copies a paragraph that already exists at HEAD into a new section and a whole judge quotes it as a repeat
- **THEN** that finding is reported
