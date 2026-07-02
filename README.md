# faith


> [!WARNING]
> Faith is in active pre-alpha development and is, from a user's point of view, currently unusable for its intended purpose.
>  However, this will change soon™.
<img align="left" style="width:200px" src="https://github.com/cococry/faith/blob/main/assets/logo.png" width="200px">

**faith is an instant messenger optimized for security, 
native performance & user customization.**

The ***faith*** <ins>client</ins> is designed around native, GPU accelerated rendering
of arbitrarily large text message scrolls.

It implements a custom text layout and rendering engine, supporting
Unicode, international scripts, emoji, bidirectional text, ligatures 
and shaped glyphs.

The user interface is rendered through an optimized rendering backend abstraction over Vulkan and OpenGL. 
Windowing is handled by the implemented backends, which currently include **X11** and **Wayland**.


The ***faith*** <ins>server</ins> is designed around security & low latency. It is written from scratch in C,
supports concurrent networking, implements a custom binary protocol and uses TLS for wire security.

## Current features 

**UI related:**
- X11/Wayland windowing backend
- Vulkan rendering backend
- Modern OpenGL rendering backend
- Instanced, single-drawcall glyph rendering
- Full emoji support
- Unicode, grapheme and bidi support
- Ligatures and text shaping with HarfBuzz
- Multidirectional text paragraphs
- Paragraph layouting engine

**Network related:**
- Concurrent server written in C
- Scalable to many clients with `epoll()` mechanics
- TCP transport secured with TLS via OpenSSL
- Basic binary protocol over wire
- In-memory table of online clients
- Support for multiple devices per client
- Client-to-client message routing

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
