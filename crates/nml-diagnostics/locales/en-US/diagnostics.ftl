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
diagnostic-invalid-reference = `{ $target }` is not a reference: write `name`, `doc#name`, `doc#` or `#name`
diagnostic-invalid-variable-name = `{ $name }` is not a variable name
diagnostic-unknown-variable = `{ $name }` is not a variable of NML
diagnostic-variable-already-set = `{ $name }` is already set
    .first = first set here
diagnostic-invalid-reference-key = `{ $key }` is not a reference key: it must be a single name without blanks, `#` or `|`
diagnostic-import-outside-prologue = `{ $module }` is imported after the prologue: imports come before the first content block
diagnostic-duplicate-import = the namespace `{ $namespace }` is already imported
    .first = first imported here
diagnostic-invalid-module-path = `{ $module }` is not a module path: write `module.path.to.file`

## Workspace, build and imports (3xxx)

diagnostic-unresolved-reference = `{ $target }` does not refer to anything
diagnostic-ambiguous-reference = `{ $target }` is ambiguous: { $count } units match it
diagnostic-unknown-module = no module is named `{ $name }` in nml.toml
diagnostic-file-not-found = `{ $path }` does not exist in the workspace
diagnostic-include-cycle = including `{ $path }` here would include it in itself
diagnostic-import-cycle = importing `{ $path }` here makes it import itself
diagnostic-config-error = invalid configuration: { $message }
diagnostic-invalid-glob = invalid pattern `{ $pattern }`: { $message }
diagnostic-invalid-module-name = `{ $name }` is not a valid module name: it must be a single word
diagnostic-duplicate-module = module `{ $name }` is declared twice
    .first = first declared here

## Lua (4xxx)

diagnostic-lua-error = { $message }
diagnostic-lua-fence-language = a `:lua` block holds Lua, not `{ $language }`: leave the fence bare
