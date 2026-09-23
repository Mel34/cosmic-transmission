# cosmic-transmission

A small, COSMIC-native panel applet for controlling and monitoring [Transmission](https://transmissionbt.com/) instances.

`cosmic-transmission` provides a native COSMIC interface for controlling Transmission services, monitoring transfer activity, switching between configured Transmission instances, and opening the Transmission Web UI when more detailed torrent management is needed.

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
* Start, stop, and restart Transmission services directly from the panel
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
* Integrated local Transmission service setup and configuration
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

`cosmic-transmission` supports multiple Transmission connections.

A connection represents a Transmission instance and contains its connection details, service configuration, and polling interval.

The Settings application manages the available connections. The panel applet provides the runtime interface for selecting a connection, controlling its Transmission daemon when applicable, monitoring its state, and opening its Web UI.

### Local connections

Two local connections are provided automatically:

* **Local User** — controls `transmission-daemon.service` as a user systemd service.
* **Local System** — controls `transmission-daemon.service` as a system systemd service.

These built-in connections cannot be deleted.

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

## Transmission service management

For local connections, `cosmic-transmission` manages the standard:

`````
transmission-daemon.service
`````

service through systemd.

The Settings application can detect the existing service and determine whether its configuration is already managed by `cosmic-transmission`.

If a local service is not configured for use, the Settings application provides a **Setup** action.

For the **Local User** connection, setup creates a user-level systemd configuration for the Transmission service.

For the **Local System** connection, setup creates the required system-level configuration and configures the service to run as the specified system user.

Existing service configuration that was not created by `cosmic-transmission` is not overwritten automatically.

Once a local service is configured, the panel applet provides controls for:

* Starting Transmission
* Stopping Transmission
* Restarting Transmission

The applet also displays the current service state.

## Transmission configuration

The Settings application can configure the Transmission RPC settings used by a local service.

The configurable values are:

* RPC port
* RPC username

When applying a configuration change, `cosmic-transmission` updates the Transmission daemon configuration and restarts the service when necessary.

Remote connections use the RPC settings configured for the individual connection.

## Settings

`cosmic-transmission-settings` is a standalone COSMIC Settings application for managing Transmission connections and local service configuration.

The Settings application provides:

* Connection list
* Adding remote connections
* Removing remote connections
* Reordering connections
* Transmission host and RPC port
* RPC username and password
* systemd service scope
* Per-connection polling interval
* Local Transmission service setup and configuration

The polling interval can be set independently for each connection:

* 1 second
* 2 seconds
* 5 seconds
* 10 seconds
* 30 seconds

The default is 2 seconds.

Local connections have fixed names and service scopes. Remote connections can be named and configured by the user.

The Settings application manages the available connections and their configuration. Runtime connection selection is performed from the connection dropdown in the panel applet.

Configuration is stored using COSMIC configuration and persists across application restarts.

## Web UI

Transmission includes its own Web UI.

For a local Transmission instance using the default RPC port, the Web UI is normally available at:

`````
http://localhost:9091/
`````

The applet's **Open Web UI** action opens the Web UI associated with the currently selected connection using the system's default browser.

The Web UI remains the primary interface for detailed torrent management. `cosmic-transmission` intentionally does not attempt to recreate the Transmission torrent-management interface.

## Security considerations

### Running Transmission as your normal user

The **Local User** connection runs Transmission with the permissions of the logged-in user.

This means Transmission can access anything that the user can access. If Transmission is compromised, an attacker could potentially gain the same filesystem and operating-system access available to that user.

The **Local System** connection can instead run the service as a specified system user, allowing Transmission to run with a more restricted set of permissions.

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

`````
git clone https://github.com/Mel34/cosmic-transmission-pkgbuild.git
cd cosmic-transmission-pkgbuild
makepkg -si
`````

The package installs both the COSMIC panel applet and the standalone Settings application.

After installation, add **Transmission** to the COSMIC panel through the panel's applet configuration.

### Building from source

For development or testing, the binaries can be built directly with Cargo:

`````
cargo build --release --locked
`````

The resulting binaries are:

`````
target/release/cosmic-transmission
target/release/cosmic-transmission-settings
`````

The Arch package is recommended for normal installations because it installs the binaries and desktop integration in the appropriate system locations.

## License

GPL-3.0-only