# faith

<img align="left" style="width:200px" src="https://github.com/cococry/faith/blob/main/assets/logo.png" width="200px">

**faith is an instant messenger optimized for security, 
native performance & customization.**

The *faith* client is designed and optimized for native, GPU accelerated rendering
of arbitrarily large text message scrolls.

It implements a custom text layout and rendering engine, supporting
Unicode, international scripts, emoji, bidirectional text, ligatures 
and shaped glyphs.

The user interface is rendered through an optimized backend abstraction 
over Vulkan and OpenGL, with native X11 and Wayland windowing support.


## Features as of now

- X11/Wayland windowing backend
- Vulkan rendering backend
- Modern OpenGL rendering backend
- Instanced, single-drawcall glyph rendering
- Full emoji support
- Unicode, grapheme and bidi support
- Ligatures and text shaping with HarfBuzz
- Multidirectional text paragraphs
- Paragraph layouting engine

## Build & Run

As faith is written in Rust, use cargo to build:

```console
git clone https://github.com/cococry/faith
cd faith
cargo build --release
```

Run it directly:
```console
./target/release/faith
```

Or install it into your Cargo binary directory:
```console
cargo install --path .
faith
```

Make sure `~/.cargo/bin` is in your `PATH` env variable.

## Contributing to faith 

See the [contribution manual](https://github.com/cococry/faith/blob/main/CONTRIBUTING.md).
