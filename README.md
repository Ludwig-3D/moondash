# Moondash

Moondash is a native Rust and Slint touchscreen interface for Klipper and
[Moonraker](https://github.com/Arksine/moonraker). It runs in a Linux desktop
session or as a Wayland kiosk with Labwc. It supports print monitoring, file
browsing, job queues, printer controls, and the
[AFC Klipper add-on](https://github.com/ArmoredTurtle/AFC-Klipper-Add-On).

## Build and run

Install a current stable Rust toolchain, a C toolchain, pkg-config, and
Fontconfig development headers (`libfontconfig1-dev` on Debian). Then run:

```sh
cargo run --locked -- --app-config /path/to/moondash.cfg
```

For a kiosk, add `--fullscreen`. Moondash uses `./config.cfg` when present and
otherwise looks for `~/printer_data/config/moondash.cfg`. Settings offers the original controls and a configuration-file
editor. Both read existing Moondash configuration files and preserve their
comments and shortcut sections. The native UI honors the configured dark
mode and primary and secondary colors. Custom logos can sit alongside the config file
as `moondash_logo.png`, `.webp`, `.jpg`, or `.gif`.

At startup Moondash tries **Vulkan → OpenGL → software**. The GPU candidates use
Slint's FemtoVG renderer. The last candidate uses Slint's software renderer.
Startup logs report the renderer used. To force a particular renderer while
diagnosing a device, pass `--renderer vulkan`, `--renderer opengl`, or
`--renderer software`.

## Install the Wayland kiosk

```sh
git clone https://github.com/Ludwig-3D/moondash
cd moondash
./scripts/install.sh
```

The installer uses the Moondash APT repository and configures Labwc and the
systemd service. Debian Bookworm and Trixie packages are built for amd64,
arm64, and armhf. To build a local package on Debian:

```sh
cargo build --locked --release
./scripts/package-deb.sh
```

To remove the kiosk service and package:

```sh
sudo systemctl disable --now moondash
sudo rm /etc/systemd/system/moondash.service
sudo systemctl daemon-reload
sudo apt remove moondash
```

## License

Moondash's source code is available under the MIT License. Native release
binaries include Slint under its GPLv3 option and are distributed under GPLv3.
See [Slint's licensing terms](https://slint.dev/terms-and-conditions).
