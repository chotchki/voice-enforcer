# Spec Delta

## MODIFIED Requirements

### Requirement: Dispute hint

When the hook lists at least one finding, its output SHALL end with one line giving the command to dispute a finding (using the absolute path of the running binary) and saying that a dispute records the finding for the ruleset owner without changing this write. When the listing holds a warning, the same line SHALL say that a wrong warning needs no rewrite: dispute it and keep the text.

#### Scenario: Hint after the findings
- **WHEN** the hook denies a write
- **THEN** the reason's last line names `dispute <id> --reason` and says the write is unchanged by it

#### Scenario: Wrong warnings get kept
- **WHEN** the hook lists only warnings as context
- **THEN** the last line also says a wrong warning should be disputed and the text kept

#### Scenario: Blocks only
- **WHEN** a deny lists only blocking findings
- **THEN** the last line says nothing about warnings

### Requirement: Samples are kept for disputes

For every finding the hook lists, it SHALL save a sample under `$XDG_STATE_HOME/voice-enforcer/findings` (default `~/.local/state/voice-enforcer/findings`) holding the rule or judge id, whether it came from a rule or a judge, the severity, the file, the line, the matched text, the message, the suggestion, the prose span the finding came from and that span's code context. A judge sample SHALL also hold the model, the effort and a hash of the rubric, the model and effort being the ones that judge actually ran with.

#### Scenario: Judge sample
- **WHEN** a judge finding is listed
- **THEN** its sample names the judge, the model, the effort and the rubric hash

#### Scenario: Table cell
- **WHEN** a judge quotes the third cell of a markdown table row
- **THEN** the sample's context is that third cell, not the row's first cell

#### Scenario: Judge with its own effort
- **WHEN** a judge that sets `effort = "high"` is listed while `[judges]` says `medium`
- **THEN** its sample records `high`
