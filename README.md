# cosmic-transmission

A small, COSMIC-native panel applet for controlling and monitoring [Transmission](https://transmissionbt.com/) instances.

`cosmic-transmission` provides a native COSMIC interface for starting and stopping Transmission services, monitoring transfer activity, switching between configured Transmission instances, and opening the Transmission Web UI when more detailed torrent management is needed.

## Why?

Transmission already provides a capable headless daemon and Web UI. There is little reason to install a complete GTK or Qt torrent client simply to control it from the COSMIC desktop.

`cosmic-transmission` takes the opposite approach:

* **Transmission** handles torrenting.
* **Transmission Web UI** handles detailed torrent management.
* **cosmic-transmission** provides a small COSMIC-native control surface.
* **libcosmic** provides the desktop integration without introducing a GTK or Qt application stack.

The applet is intentionally not a replacement for the Transmission Web UI. It provides the desktop controls and status information that are useful from the panel, while leaving detailed torrent management to Transmission itself.

## Features

* COSMIC-native panel applet
* Start and stop Transmission services directly from the panel
* Monitor Transmission daemon state
* View current download and upload rates
* View downloading, seeding, and active torrent counts
* Open the Transmission Web UI in the default browser
* Support for multiple Transmission connections
* Switch between connections from the applet
* Built-in local User and System connections
* Configurable remote Transmission connections
* Per-connection polling intervals
* Optional RPC username and password for remote connections
* User or System systemd service scope
* Standalone COSMIC Settings application
* Persistent configuration using COSMIC configuration
* Password storage through the system credential backend

## Requirements

* COSMIC Desktop
* Transmission
* `systemd`
* `xdg-utils`

The packaged version also depends on the COSMIC applet infrastructure provided by the distribution.

## Connections

Version 0.4 introduces connection management.

A connection represents a Transmission instance and contains its connection details, service configuration, and polling interval. One connection is selected as the active connection, and the panel applet operates on that connection.

### Local connections

Two local connections are provided automatically:

* **Local User** — controls `transmission-daemon.service` as a user systemd service.
* **Local System** — controls `transmission-daemon.service` as a system systemd service.

These built-in connections cannot be deleted.

The Local User connection is the default active connection.

### Remote connections

Remote Transmission instances can be added from the Settings application.

A remote connection can specify:

* Connection name
* Host
* RPC port
* Username
* Password
* Polling interval

Remote connections do not control a local systemd service. They communicate with the configured Transmission RPC endpoint.

## Setting up a local Transmission service

For the **Local User** connection, `cosmic-transmission` expects a user-level systemd service named:

```text
transmission-daemon.service
```

Distributions may provide a suitable user unit themselves. If not, create a user service unit appropriate for your Transmission installation under:

```text
~/.config/systemd/user/
```

For example:

```ini
[Unit]
Description=Transmission BitTorrent daemon
After=network-online.target

[Service]
ExecStart=/usr/bin/transmission-daemon --foreground
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
```

Save it as:

```text
~/.config/systemd/user/transmission-daemon.service
```

The `ExecStart` path and arguments may differ depending on how Transmission is installed on your system.

After adding or changing the unit:

```bash
systemctl --user daemon-reload
```

Once the service unit is available, the **Local User** connection can start and stop Transmission directly from the COSMIC panel.

### System service

The **Local System** connection is intended for a system-level `transmission-daemon.service`.

The service must be available to the system systemd manager and configured appropriately for the Transmission installation.

### systemd lingering

If Transmission should continue running without an active graphical login session, user systemd lingering may be enabled:

```bash
loginctl enable-linger
```

This is optional and is only necessary if you want the user-level daemon to continue running independently of your graphical login session.

## Settings

`cosmic-transmission-settings` is a standalone COSMIC Settings application for managing Transmission connections.

The Settings application provides:

* Connection list and selection
* Adding remote connections
* Removing remote connections
* Reordering connections
* Selecting the active connection
* Transmission host and RPC port
* RPC username and password
* systemd service scope
* Per-connection polling interval

The polling interval can be set independently for each connection:

* 1 second
* 2 seconds
* 5 seconds
* 10 seconds
* 30 seconds

The default is 2 seconds.

Local connections have fixed names and service scopes. Remote connections can be named and configured by the user.

Configuration is stored using COSMIC configuration and persists across application restarts.

## Web UI

Transmission includes its own Web UI.

For a local Transmission instance using the default RPC port, the Web UI is normally available at:

```text
http://localhost:9091/
```

The applet's **Open Web UI** action opens the Web UI associated with the active connection using the system's default browser.

The Web UI remains the primary interface for detailed torrent management. `cosmic-transmission` intentionally does not attempt to recreate the Transmission torrent-management interface.

## Security considerations

### Running Transmission as your normal user

The **Local User** connection runs Transmission as your normal user account.

That is convenient, but it has an important security consequence:

> Transmission runs with the permissions of your normal user account.

If Transmission or an exposed RPC/Web UI endpoint is compromised, an attacker potentially gains the same filesystem and operating-system permissions available to that user.

This is different from running Transmission under a dedicated, restricted system account.

For a personal desktop where Transmission only accesses directories you explicitly use for torrents, running it as your user can be a reasonable trade-off. However, understand the implications before exposing the daemon beyond your own machine.

### Keep RPC local unless you need remote access

For a local installation, keep Transmission's RPC interface bound to the local machine unless remote access is actually required.

If remote access is required:

* enable RPC authentication;
* restrict which addresses may connect;
* use an appropriate firewall;
* avoid exposing the RPC port directly to the Internet.

**Do not expose an unauthenticated Transmission RPC interface to an untrusted network.**

For remote connections configured in `cosmic-transmission`, use the same precautions appropriate to the network between the desktop and the Transmission instance.

## Design goals

`cosmic-transmission` aims to be:

* **COSMIC-native** — built with `libcosmic`;
* **lightweight** — no duplicate torrent-management interface;
* **desktop-integrated** — controlled directly from the panel;
* **multi-instance aware** — multiple Transmission connections can be configured and switched from the desktop;
* **daemon-oriented** — Transmission does the actual torrenting;
* **GTK/Qt-independent** — no additional desktop toolkit is required just to control Transmission;
* **simple** — the Web UI remains available when more control is needed.

The project intentionally leaves detailed torrent management to Transmission rather than trying to become another Transmission client.

## Installation

### Arch Linux

An Arch Linux PKGBUILD is provided separately:

```bash
git clone https://github.com/Mel34/cosmic-transmission-pkgbuild.git
cd cosmic-transmission-pkgbuild
makepkg -si
```

The package installs both the COSMIC panel applet and the standalone Settings application.

After installation, add **Transmission** to the COSMIC panel through the panel's applet configuration.

### Building from source

For development or testing, the binaries can be built directly with Cargo:

```bash
cargo build --release --locked
```

The resulting binaries are:

```text
target/release/cosmic-transmission
target/release/cosmic-transmission-settings
```

The Arch package is recommended for normal installations because it installs the binaries and desktop integration in the appropriate system locations.

## License

GPL-3.0-only
