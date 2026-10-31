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
        [Positive] a positive integer
        [Number] a number
        [Bool] `true` or `false`
       *[other] one of { $expected }
    }
diagnostic-missing-option = missing option `{ $name }`

## Built-in language (2xxx)

diagnostic-heading-too-deep = a heading has at most { $max } levels, not { $level }
diagnostic-duplicate-reference = `{ $name }` is already defined in this document
    .first = first defined here
diagnostic-second-reference-name = a heading has one reference name, not `{ $name }` too
    .first = first named here
diagnostic-unknown-code-language = `{ $language }` is not a known language: the code is not highlighted
diagnostic-unknown-media-type = cannot tell whether `{ $url }` is an image, a video or an audio file: set its `type`
diagnostic-unknown-callout = `{ $kind }` is not a kind of callout: use one of { $kinds }
diagnostic-unknown-layout = `{ $name }` is not a layout: use one of { $layouts }
diagnostic-layout-parts = the `{ $name }` layout has { $expected ->
    [one] one part
   *[other] { $expected } parts
}, not { $found }
diagnostic-not-a-table = `:table` applies to a table, which starts with `|`
diagnostic-duplicate-table-name = a table is already named `{ $name }` for scripts
    .first = first named here
diagnostic-fence-language = `{ $language }` is not the language of this command: write `{ $expected }`, or nothing
diagnostic-render-error = { $message }
diagnostic-missing-runner = a `:run` block names its runner after its fence: ```R
diagnostic-duplicate-run-name = a run is already named `{ $name }`
    .first = first named here
diagnostic-unknown-run = no run before this is named `{ $name }`: name one with `:run #{ $name }`
diagnostic-missing-run-output = the run `{ $name }` wrote no file `{ $file }`
diagnostic-ambiguous-run-output = { $count ->
    [0] the run `{ $name }` wrote no file
   *[other] the run `{ $name }` wrote { $count } files: write `run:{ $name }/<file>` with one of { $files }
}
diagnostic-progress-outside = `[/]` and `[%]` count the tasks of their list item or heading, and this is in neither
diagnostic-undefined-footnote = no note is defined for `[^{ $name }]`: write `[^{ $name }]: text` at the start of a line
diagnostic-unused-footnote = the note `{ $name }` is never referred to: write `[^{ $name }]` where it applies
diagnostic-duplicate-footnote = the note `{ $name }` is already defined
    .first = first defined here
diagnostic-listing-lines = `{ $lines }` is not a range of lines of the file, which has { $count } lines: write `10-20`, `10-` or `-20`
diagnostic-unknown-region = `{ $path }` has no region `{ $region }`: mark it with lines holding `nml:begin { $region }` and `nml:end { $region }`
diagnostic-listing-selection = a listing takes `lines` or `region`, not both
diagnostic-unknown-run-data = `{ $name }` is neither a table named with `:table[lua={ $name }]` before this, nor a variable
diagnostic-invalid-tangle-path = `{ $path }` leaves the workspace: code is tangled to files of the workspace
diagnostic-tangle-conflict = `{ $path }` is also tangled from `{ $other }`: a file is tangled from one document
diagnostic-invalid-timestamp = `{ $timestamp }` is not a timestamp: write `<YYYY-MM-DD>`, then optionally a weekday and a time, `<2026-11-20 Fri 14:00>`
diagnostic-wrong-weekday = this date is a { $actual }, not a { $written }
    .fix = Write { $actual }
diagnostic-invalid-reference = `{ $target }` is not a reference: write `name`, `doc#name`, `doc#` or `#name`
diagnostic-invalid-variable-name = `{ $name }` is not a variable name
diagnostic-unknown-variable = `{ $name }` is not a variable of NML
diagnostic-variable-already-set = `{ $name }` is already set
diagnostic-undefined-variable = `{ $name }` is not set
diagnostic-variable-cycle = `{ $name }` refers to itself
    .first = first set here
diagnostic-invalid-reference-key = `{ $key }` is not a reference key: it must be a single name without blanks, `#` or `|`
diagnostic-invalid-output-path = `{ $path }` is not an output path: it must be a relative path ending with `.html`, outside `assets/`, without `.` or `..` parts
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
diagnostic-output-conflict = `{ $output }` is also the output of `{ $other }`: set `nml.output` to write one of them elsewhere
diagnostic-reserved-static-file = `{ $path }` in the static directory would replace a file of the build: `assets/` and `nml-code.css` are the build's
diagnostic-missing-static-directory = the static directory `{ $path }` does not exist
diagnostic-unknown-runner = `{ $runner }` is not a runner of nml.toml: declare it in `[run.{ $runner }]`{ $runners ->
    [none] {""}
   *[other] , or use one of { $runners }
}
diagnostic-run-failed = the run failed: { $message }
diagnostic-unknown-code-theme = `{ $theme }` is not a code theme: use one of { $themes }
diagnostic-duplicate-module = module `{ $name }` is declared twice
    .first = first declared here

## Lua (4xxx)

diagnostic-lua-error = { $message }
diagnostic-lua-fence-language = a `:lua` block holds Lua, not `{ $language }`: leave the fence bare
diagnostic-lua-rule-outside-prologue = the rule `{ $rule }` is defined after the prologue: define rules in the prologue or in a module
diagnostic-invalid-lua-rule = the rule `{ $rule }` cannot be added: { $message }
diagnostic-deferred-module-not-imported = `{ $module }` is not imported: a deferred function comes from a module the document imports
