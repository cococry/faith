# faith

<img align="left" style="width:260px" src="https://github.com/cococry/faith/blob/main/assets/logo.png" width="240px">

**faith is an instant messenger optimized for security, 
native performance & customization**

The faith client is highly optimized for native, GPU accelerated rendering
of arbitrarily large text message scrolls.

faith implements a custom text-rendering and -layouting engine, supporting
everything from Unicode to international scripts. The user interface is 
rendered through a Vulkan & OpenGL layer, interfacing with the GPU directly.

---

## Features as of now

- X11/Wayland windowing backend
- OpenGL rendering backend
- Instanced, single drawcall glyph rendering
- Colored glyph support
- Unicode, grapheme & bidi support
- Ligatures & text shaping with HarfBuzz

## Build & Run

As faith is written in Rust, use cargo to build and run faith:
```console
cargo build
cargo run
```
