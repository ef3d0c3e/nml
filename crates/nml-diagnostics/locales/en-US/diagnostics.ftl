# Messages of `DiagnosticKind`. Each message is a kind's title; its attributes are labels and
# fix titles. A message's variables are the fields of its variant.

## Grammar engine (1xxx)

diagnostic-expected-token = expected `{ $token }`
    .opened = { $style } opened here
    .ended = ended here without `{ $token }`
    .fix-escape = Escape the opening delimiter
diagnostic-unexpected-token = unexpected `{ $token }`
diagnostic-expected-atom = expected { $atom ->
        [Word] a word
        [Number] a number
        [LineStart] the start of a line
        [LineEnd] the end of the line
       *[other] { $atom }
    }
diagnostic-nesting-too-deep = constructs are nested more than { $limit } deep
diagnostic-unknown-option = unknown option `{ $name }`
diagnostic-duplicate-option = option `{ $name }` is given twice
    .first = first given here
diagnostic-invalid-option-value = invalid value for `{ $name }`: expected { $expected ->
        [Text] text
        [Integer] an integer
        [Number] a number
        [Bool] `true` or `false`
       *[other] one of { $expected }
    }
diagnostic-missing-option = missing option `{ $name }`

## Built-in language (2xxx)

diagnostic-heading-too-deep = a heading has at most { $max } levels, not { $level }
diagnostic-duplicate-reference = `{ $name }` is already defined in this document
    .first = first defined here

## Lua (4xxx)

diagnostic-lua-error = { $message }
