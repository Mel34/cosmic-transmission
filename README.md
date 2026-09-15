# cosmic-transmission

A small COSMIC Desktop applet for monitoring and controlling Transmission daemon instances.

The applet provides a native COSMIC panel interface for controlling Transmission, monitoring its state and basic transfer statistics, and opening the Transmission Web UI.

## Screenshots
<img width="379" height="361" alt="Panel applet" src="https://github.com/user-attachments/assets/d748800a-9a35-49a4-9a1a-4ce9ef284de6" />

Panel applet

<img width="502" height="439" alt="Settings" src="https://github.com/user-attachments/assets/84047666-d06b-471d-b26c-57c994522c4c" />

Settings

## Why?

Transmission already provides a perfectly good headless daemon and Web UI. There is little reason to install a complete GTK or Qt client simply to control it from COSMIC.

`cosmic-transmission` takes the opposite approach:

* Transmission daemon handles torrenting.
* Transmission Web UI handles detailed torrent management.
* `cosmic-transmission` provides a small COSMIC-native control surface.
* `libcosmic` provides the GUI integration without introducing a GTK or Qt desktop stack.

This keeps the desktop integration focused on what COSMIC actually needs while leaving Transmission itself responsible for torrent management.

There is no attempt to recreate the Transmission GUI.

## Features

* Native COSMIC panel applet
* Download and upload speed monitoring
* Downloading, seeding, and active torrent counts
* Start / stop control for the Transmission daemon
* User and system systemd service control
* Configurable Transmission RPC host and port
* Native COSMIC settings window
* Open Transmission Web UI from the applet
* Persistent configuration
* GTK/Qt-independent

## Requirements

* COSMIC Desktop
* Transmission
* `systemd`
* `xdg-utils`

The packaged version also depends on the COSMIC applet infrastructure provided by the distribution.

## Transmission service

`cosmic-transmission` can control Transmission through either a user-level or system-level systemd service.

The default service name is:

```text
transmission-daemon.service
```

### User service

If Transmission is run as a user service, the unit normally lives under:

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

If Transmission should continue running without an active login session, user systemd lingering may be enabled:

```bash
loginctl enable-linger
```

This is optional and is only necessary if you want the daemon to run independently of your graphical login session.

### System service

Transmission may also be installed as a system-level service:

```text
transmission-daemon.service
```

When configured for system scope, `cosmic-transmission` uses systemd's D-Bus interface and polkit to control the service without requiring the applet itself to run with elevated privileges.

The service scope can be selected from the applet's Settings window.

## Configuration

The **Settings** entry in the applet opens the native `cosmic-transmission-settings` application.

Current settings include:

* Transmission RPC host
* Transmission RPC port
* Service scope: User or System

The RPC connection and service-control scope are separate concepts. The RPC endpoint may be configured independently of which local systemd service is being controlled.

## Security considerations

### Running Transmission as your normal user

Transmission can run with the permissions of your normal user account when using a user-level systemd service.

If Transmission or an exposed RPC/Web UI endpoint is compromised, an attacker potentially gains the same filesystem and operating-system permissions available to that user.

Running Transmission under a dedicated, restricted system account can provide stronger isolation.

For a personal desktop where Transmission only accesses directories you explicitly use for torrents, running it as your user can be a reasonable trade-off. However, understand the implications before exposing the daemon beyond your own machine.

### Keep RPC local unless you need remote access

The default local RPC endpoint is:

```text
http://localhost:9091/transmission/rpc
```

The Transmission Web UI is normally available on the same port.

If you only need local access, configure Transmission's RPC server to listen only on the local machine and avoid exposing it to your LAN or the Internet.

If remote access is required:

* enable RPC authentication;
* restrict which addresses may connect;
* use an appropriate firewall;
* avoid exposing the RPC port directly to the Internet.

Do not expose an unauthenticated Transmission RPC interface to an untrusted network.

## Web UI

Transmission includes its own Web UI.

Once the daemon is running, it is normally available at:

```text
http://localhost:9091/
```

The **Open Web UI** entry in `cosmic-transmission` opens the configured Web UI address in the system's default browser.

The Web UI remains the primary interface for detailed torrent management. The applet is intentionally not intended to replace it.

## The applet

The panel icon reflects the current Transmission daemon state.

| State    | Icon                            |
| -------- | ------------------------------- |
| Checking | `network-server-symbolic`       |
| Running  | `network-receive-symbolic`      |
| Stopped  | `network-disconnected-symbolic` |
| Error    | `network-error-symbolic`        |

Clicking the panel icon opens the popup.

The popup provides:

* current daemon state;
* Start / Stop control;
* download and upload rates;
* number of downloading torrents;
* number of seeding torrents;
* number of active torrents;
* Open Web UI;
* Settings.

Transmission statistics are obtained through the Transmission RPC interface.

The applet monitors the configured systemd service and tracks service transitions such as starting and stopping.

## Installation

### Arch Linux

An Arch Linux package is maintained in the [`cosmic-transmission-pkgbuild`](https://github.com/Mel34/cosmic-transmission-pkgbuild) repository.

### From source

Clone the repository and build with Cargo:

```bash
git clone https://github.com/Mel34/cosmic-transmission.git
cd cosmic-transmission
cargo build --release --locked
```

The build produces two executables:

```text
target/release/cosmic-transmission
target/release/cosmic-transmission-settings
```

Install both binaries and the desktop entry according to the conventions of your distribution.


### From source

Clone the repository and build with Cargo:

```bash
git clone https://github.com/Mel34/cosmic-transmission.git
cd cosmic-transmission
cargo build --release --locked
```

The build produces two executables:

```text
target/release/cosmic-transmission
target/release/cosmic-transmission-settings
```

Install both binaries and the desktop entry according to the conventions of your distribution.

## Design goals

`cosmic-transmission` aims to be:

* **COSMIC-native** — built with `libcosmic`;
* **lightweight in functionality** — no duplicate torrent-management UI;
* **desktop-integrated** — controlled directly from the panel;
* **daemon-oriented** — Transmission does the actual work;
* **GTK/Qt-independent** — no need to install another desktop toolkit just to control Transmission;
* **simple** — the Web UI remains available when more control is needed.

The project intentionally leaves the torrent-management interface to Transmission rather than trying to become another Transmission client.

## Roadmap

### 0.4

Planned:

* Multiple Transmission connections
* Local user and system Transmission instances
* Remote Transmission instances
* Per-connection settings
* Reorderable connections
* Secret Service credential storage
* Authenticated remote RPC
* Improved connection and error states

The goal for 0.4 is to make each Transmission instance a first-class connection while keeping the applet focused on status, daemon control, and quick access to the Web UI.

## License

GPL-3.0-only
