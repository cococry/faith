# faith

> [!WARNING]
> faith is in active pre-alpha development and is, from a user's point of view, currently unusable for its intended purpose.
>  However, this will change soon™.
<img align="left" style="width:200px" src="https://github.com/cococry/faith/blob/main/assets/logo.png" width="200px">

**faith is an instant messenger optimized for security, 
native performance & user customization.**

The ***faith*** [<ins>UI client</ins>](https://github.com/cococry/faith/tree/main/faith-ui) is designed around native, GPU accelerated rendering
of arbitrarily large text message scrolls.

It implements a custom text layout and rendering engine, supporting
Unicode, international scripts, emoji, bidirectional text, ligatures 
and shaped glyphs.

The user interface is rendered through an optimized rendering backend abstraction over Vulkan and OpenGL. 
Windowing is handled by the implemented backends, which currently include **X11** and **Wayland**.

faith uses a **custom immediate-mode UI framework** whose rendering is event-driven to limit power consumption.


The ***faith*** [<ins>server</ins>](https://github.com/cococry/faith/tree/main/faith-server) is designed around security & low latency. It is written from scratch in C,
supports concurrent networking, implements a custom binary protocol and uses TLS for wire security.

![faith UI (dark)](assets/faith-ui.png#gh-dark-mode-only)
![faith UI (light)](assets/faith-ui-light.png#gh-light-mode-only)

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
- Secure device-linking system
- Client-to-client message routing

## Build & Run

To run faith, you will first need to start the local development server. This will run two API servers
and the database server on your local machine. For the script to work, you will need a functional `docker`
setup with `docker compose` available.

Start the server:
```console
git clone https://github.com/cococry/faith
cd faith
./scripts/boot_system.sh
```

Once the server is running, start the GUI client with the default options.

From the project root:
```console
cd faith-ui
cargo run
```

## Contributing to faith 

See the [contribution manual](https://github.com/cococry/faith/blob/main/CONTRIBUTING.md).
