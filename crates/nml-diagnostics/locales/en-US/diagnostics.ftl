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

## Lua (4xxx)

diagnostic-lua-error = { $message }
