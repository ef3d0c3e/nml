# NML -- Not a markup language!

Documentation is available [here](https://ef3d0c3e.github.io/nml/readme/Getting%20Started.html)

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

NML statically compiles liblua5.4 to use the lua features.

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
 - [x] LSP
 - [ ] Complete Lua api
 - [ ] Documentation
 - [ ] Table
 - [ ] LaTeX output

# License

NML is licensed under the GNU AGPL version 3 or later. See [LICENSE.md](LICENSE.md) for more information.
License for third-party dependencies can be accessed via `cargo license`
