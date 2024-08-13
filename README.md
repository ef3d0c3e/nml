# NML -- Not a markup language!

Currently a work in progress, expect features and fixes to arrive soon!

# Requirements

Some features requires external dependencies to work.

## LaTeX rendering for HTML

We ship a modified version of `latex2svg` by Matthias C. Hormann.
The modified program can be found in [third/latex2svg](third/latex2svg) and is licensed under MIT.

The installation instructions can be found on [latex2svg's repository](https://github.com/Moonbase59/latex2svg).

## Graphviz rendering

To render Graphviz graph (i.e `[graph]...[/graph]`),
you need to install the `dot` program from [Graphviz](https://graphviz.org/).

## Lua kernels

To execute Lua kernels you need to install `liblua` version 5.4.
Support for a statically linked Lua may be added in the future.

# Compiling

```
cargo build --release --bin nml
```

# Features roadmap

 - [x] Paragraphs
 - [x] LaTeX rendering
 - [x] Graphviz rendering
 - [x] Media
 - [x] References
 - [x] Navigation
 - [x] Cross-Document references
 - [ ] Complete Lua api
 - [ ] Documentation
 - [ ] Table
 - [ ] LaTeX output
 - [ ] LSP

# License

NML is licensed under the GNU AGPL version 3 or later. See [LICENSE.md](LICENSE.md) for more information.
License for third-party dependencies can be accessed via `cargo license`
