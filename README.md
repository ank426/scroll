# scroll

Use your phone as a wireless scroll wheel on Linux.

## Install

```bash
cargo install --git https://github.com/ank426/scroll
```

Or clone and install:

```bash
git clone https://github.com/ank426/scroll
cd scroll
cargo install --path .
```

## Setup

Scroll needs write access to `/dev/uinput` to create a virtual mouse-wheel input device. To grant your user permanent access:

```sh
sudo groupadd -r uinput
sudo usermod -aG uinput $USER
echo 'KERNEL=="uinput", MODE="0660", GROUP="uinput", OPTIONS+="static_node=uinput"' \
| sudo tee /etc/udev/rules.d/60-scroll.rules
```

Then reboot, or run:

```sh
sudo udevadm control --reload
sudo udevadm trigger
```

Log out and back in for the new group membership to take effect. Users in the `uinput` group can synthesize input events system-wide, so do not add untrusted users to it.

## Usage

Start the server and show a QR code:

```bash
scroll --qr
```

Scan the code, or open the displayed URL on a phone connected to the same network. Swipe vertically to scroll on the host.

## Options

```text
> scroll --help
Use your phone to scroll

Usage: scroll [OPTIONS]

Options:
  -p, --port <PORT>                Port to listen on [default: 12687]
  -s, --sensitivity <SENSITIVITY>  Scroll sensitivity multiplier [default: 6]
  -q, --qr [<QR>]                  Show QR code in terminal [default: false] [possible values: true, false]
  -i, --iface <IFACE>              Network interface to use for the displayed URL
  -h, --help                       Print help
```

Without `--iface`, scroll listens on all interfaces and displays the detected local IP. With `--iface`, it listens only on that interface.
