# Reaching it from outside

HTTPS, the http→https redirect, the Synology reverse proxy and the relay. Back to the [README](../README.md).

## Synology reverse proxy

The simplest way in from outside your home: use the Synology reverse proxy (Control Panel → Login
Portal → Advanced → Reverse Proxy) with HTTPS, and put that address into Settings →
"Address used in invite links" so invite links point to it.

## Settings → Network

Under **Settings → Network**:

- **HTTPS**: enter a certificate file (PEM, with the chain) and its key file (PEM), then restart.
  The port then speaks HTTPS instead of HTTP. Both files are checked when you save (a key that
  does not belong to the certificate is refused). A renewed certificate (certbot, acme.sh) is
  picked up within about 30 seconds, without a restart; if the new files are wrong the old
  certificate stays in use and the log says why. There is no built-in Let's Encrypt client.
  `--tls-cert` / `--tls-key` (or `LICHTSPIEL_TLS_CERT` / `LICHTSPIEL_TLS_KEY`) override the settings.
- **Pass http:// on to https:// from port**: a second port (e.g. 80) that answers plain
  http:// with a redirect to the https:// address. Takes effect after a restart.
- **Relay**: when the router can't forward a port, run `lichtspiel --relay --relay-secret <secret>`
  on a computer that can be reached from the internet (`--relay-public` is where callers come in,
  default `0.0.0.0:8484`; `--relay-tunnel` is where the home server calls in, default
  `0.0.0.0:8485`; also `LICHTSPIEL_RELAY_*`). Then enter that computer's `name:8485` and the
  secret in the settings. The home server keeps a few connections open to the relay and hands
  each caller to its own web server, so the relay only passes bytes on. The secret itself is
  never sent. The traffic is private only when the home server speaks HTTPS with a
  certificate for the relay's name; with plain HTTP everything crosses the internet readable.
  One relay serves one home server. Tested on one computer only, not over a real network.

