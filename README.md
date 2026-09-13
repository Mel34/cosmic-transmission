# cosmic-transmission

A small COSMIC Desktop applet for controlling a user-level [Transmission](https://transmissionbt.com/) daemon.

The applet provides a native COSMIC panel interface for starting and stopping Transmission, monitoring its state, viewing basic transfer statistics, and opening the Transmission Web UI.

## Why?

Transmission already provides a perfectly good headless daemon and Web UI. There is little reason to install a complete GTK or Qt client simply to control it from COSMIC.

`cosmic-transmission` takes the opposite approach:

* **Transmission daemon** handles torrenting.
* **Transmission Web UI** handles detailed torrent management.
* **cosmic-transmission** provides a small COSMIC-native control surface.
* **libcosmic** provides the GUI integration without introducing a GTK or Qt desktop stack.

This keeps the desktop integration focused on what COSMIC actually needs while leaving Transmission itself responsible for torrent management.

There is no attempt to recreate the Transmission GUI here.

## Requirements

* COSMIC Desktop
* Transmission
* `systemd` with a user service instance
* `xdg-utils`

The packaged version also depends on the COSMIC applet infrastructure provided by the distribution.

## Setting up Transmission as a user service

`cosmic-transmission` expects Transmission to run as a user-level systemd service named:

```
transmission-daemon.service
```

Distributions may provide a suitable user unit themselves. If not, create a user service unit appropriate for your Transmission installation under:

```
~/.config/systemd/user/
```

For example:

```
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

```
~/.config/systemd/user/transmission-daemon.service
```

The `ExecStart` path and arguments may differ depending on how Transmission is installed on your system.

After adding or changing the unit, reload the user systemd manager:

```
systemctl --user daemon-reload
```

Once the service unit is installed and the user systemd manager has been reloaded, `cosmic-transmission` can start and stop the service directly from its panel popup.


If Transmission should continue running without an active login session, user systemd lingering may be enabled:

```
loginctl enable-linger
```

This is optional and is only necessary if you want the daemon to run independently of your graphical login session.

## Security considerations

### Running Transmission as your normal user

This applet is designed around a **user-level Transmission daemon**.

That is convenient, but it has an important security consequence:

> Transmission runs with the permissions of your normal user account.

If Transmission or an exposed RPC/Web UI endpoint is compromised, an attacker potentially gains the same filesystem and operating-system permissions available to that user.

This is different from running Transmission under a dedicated, restricted system account.

For a personal desktop where Transmission only accesses directories you explicitly use for torrents, running it as your user can be a reasonable trade-off. However, understand the implications before exposing the daemon beyond your own machine.

### Keep RPC local unless you need remote access

The applet communicates with Transmission through its RPC interface on:

```
http://localhost:9091/transmission/rpc
```

The Web UI is normally available on port `9091` as well.

If you only need local access, configure Transmission's RPC server to listen only on the local machine and avoid exposing it to your LAN or the Internet.

If remote access is required:

* enable RPC authentication;
* restrict which addresses may connect;
* use an appropriate firewall;
* avoid exposing the RPC port directly to the Internet.

**Do not expose an unauthenticated Transmission RPC interface to an untrusted network.**

## Web UI

Transmission includes its own Web UI.

Once the daemon is running, open:

```
http://localhost:9091/
```

The **Open Web UI** button in `cosmic-transmission` opens this address in the system's default browser.

The Web UI remains the primary interface for detailed torrent management. The applet is intentionally not intended to replace it.

## The applet

The panel icon reflects the current Transmission daemon state:

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
* Open Web UI button.

Transmission statistics are obtained through the Transmission RPC interface.

The applet periodically checks the user systemd service and tracks service transitions such as starting and stopping.

## Design goals

`cosmic-transmission` aims to be:

* **COSMIC-native** — built with `libcosmic`;
* **lightweight in functionality** — no duplicate torrent-management UI;
* **desktop-integrated** — controlled directly from the panel;
* **daemon-oriented** — Transmission does the actual work;
* **GTK/Qt-independent** — no need to install another desktop toolkit just to control Transmission;
* **simple** — the Web UI remains available when more control is needed.

The project intentionally leaves the torrent-management interface to Transmission rather than trying to become another Transmission client.

## License

GPL-3.0-only
