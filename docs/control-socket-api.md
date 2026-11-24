# Control socket API

`smartconnectd` serves a local Unix socket that every desktop front-end uses: the CLI, the GTK app,
the Noctalia plugin, the quick-reply window, and the helpers (`smartconnect-mirror`,
`smartconnect-crosscontrol`, `smartconnect-display`, `smartconnect-webcam`). This page is the
complete reference. It is written from `src/control.rs`, so update it in the same change whenever
that file changes.

- [Connecting](#connecting) · [Framing](#framing) · [Replies and errors](#replies-and-errors) · [Examples](#examples)
- [Command summary](#command-summary)
- Commands: [`status`](#status) · [`subscribe`](#subscribe) · [`connect`](#connect) · [`disconnect`](#disconnect) ·
  [`pair`](#pair) · [`watch`](#watch) (with [local events](#local-events-service--1)) · [`send`](#send) ·
  [`reply`](#reply) · [`notif_mute`](#notif_mute) · [SMS commands](#sms-commands)
- Objects: [the status object](#the-status-object) · [SMS objects](#sms-objects)
- [Error messages](#error-messages)
- [The stream host socket](#the-stream-host-socket) (`smartconnect-mirror --host`)
- [Compatibility](#compatibility)

## Connecting

| | |
|---|---|
| Path | `$XDG_RUNTIME_DIR/smartconnectd.sock`, or the system temp directory (`/tmp`) if `XDG_RUNTIME_DIR` is unset |
| Type | Unix stream socket |
| Lifetime | Created when the daemon starts, after it removes any stale file. A daemon restart replaces it, so clients must reconnect. The GTK app retries every 2 s and the Noctalia plugin every 3 s. |
| Access | No authentication. Anyone who can connect gets full control, so access is governed by file permissions. `$XDG_RUNTIME_DIR` is private to your user (mode 0700), and the socket itself is created with the daemon's umask. See [Security](security.md#local-exposure). |

The CLI is a thin wrapper around the plain-word commands:

```sh
smartconnectd status        # sends "status\n", prints the reply line
smartconnectd subscribe     # prints lines until Ctrl+C
```

## Framing

- Every request is **one line of UTF-8 ending in `\n`**. Every reply is **one line of JSON** ending
  in `\n`.
- There are two request syntaxes:
  - a **plain word**: `status`, `subscribe`, `connect`, `disconnect`, `pair`. Surrounding whitespace
    is ignored.
  - a **JSON object** with a `cmd` field, such as `{"cmd":"reply","key":"…","text":"…"}`.
  The daemon first tries to parse each line as JSON. Any line that parses as JSON is treated as a
  JSON command, so `"status"` (with quotes) or `42` gives `unknown command ""`.
- **Empty lines** are ignored and get no reply.
- **One connection can carry many requests.** They are handled one after another, and each gets its
  reply before the next line is read. `connect` can take up to about a minute, and the next request
  waits for it.
- **Streaming commands take over the connection.** After `subscribe` or `watch` the daemon only
  writes; it doesn't read further requests on that connection. Open a second connection for
  commands, as the helpers do: one `watch` connection plus one connection for `send`.
- Invalid UTF-8 closes the connection.

## Replies and errors

- Action commands reply `{"message":"…","ok":true|false}`. Some add fields, for example `qr` or
  `requestId`.
- Data commands reply with their data: `status` with [the status object](#the-status-object),
  `sms_state` with a snapshot, `notif_mute` with `{"muted":[…],"ok":true}`.
- **Every error** has the form `{"message":"<reason>","ok":false}`. The connection stays open after
  an error. All reasons are listed in [Error messages](#error-messages).
- JSON object keys in replies mostly come out in alphabetical order. The status object, SMS messages
  and the `watch` acknowledgement keep a fixed field order instead. Never rely on key order.
- `"ok":true` on a command that talks to the phone means the message was **queued for the current
  session**, not that the phone acted on it. The phone's answers, if any, arrive through
  [`watch`](#watch), or are reflected in `status` or `sms_state`.

SMS/MMS unread status follows phone-pushed service 100 `cmd 1018` messages. There is no
periodic metadata query. `subscribe` receives changes when the phone reports them, including
zero; a local phone-side read may leave the PC count unchanged until another status arrives.
Explicit SMS commands and generic `send` requests remain available when needed.

## Examples

With `socat`:

```sh
S=$XDG_RUNTIME_DIR/smartconnectd.sock
echo status | socat - UNIX-CONNECT:$S
echo '{"cmd":"notif_mute"}' | socat - UNIX-CONNECT:$S

# Slow commands: by default socat stops waiting 0.5 s after its input ends. Give it time.
echo connect | socat -t 90 - UNIX-CONNECT:$S

# Streams: keep the connection open for as long as you want output.
echo subscribe | socat -t 86400 - UNIX-CONNECT:$S
echo '{"cmd":"watch","services":[25]}' | socat -t 86400 - UNIX-CONNECT:$S
```

With OpenBSD netcat (`nc -U`; `-N` closes the sending side at end of input, so `nc` exits after
the reply):

```sh
echo status | nc -N -U "$XDG_RUNTIME_DIR/smartconnectd.sock"
echo '{"cmd":"watch","services":[5,37]}' | nc -U "$XDG_RUNTIME_DIR/smartconnectd.sock"   # streams until Ctrl+C
```

With Python, watching device status while sending a command on a second connection:

```python
import json, os, socket

path = os.path.join(os.environ["XDG_RUNTIME_DIR"], "smartconnectd.sock")

def connect():
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(path)
    return s.makefile("rw", encoding="utf-8")

cmd = connect()
cmd.write("status\n"); cmd.flush()
print("connected:", json.loads(cmd.readline())["connected"])

watch = connect()
watch.write(json.dumps({"cmd": "watch", "services": [25]}) + "\n"); watch.flush()
print(watch.readline().strip())                # {"ok":true,"message":"watching"}
for line in watch:
    frame = json.loads(line)
    print(frame["service"], frame["data"].get("cmd"))
```

## Command summary

| Command | Syntax | Needs a connected phone | Reply |
|---|---|---|---|
| [`status`](#status) | plain | no | the status object |
| [`subscribe`](#subscribe) | plain | no | the status object, then one per change (stream) |
| [`connect`](#connect) | plain | no | `ok` + `message` |
| [`disconnect`](#disconnect) | plain | yes (`ok:false` otherwise) | `ok` + `message` |
| [`pair`](#pair) | plain | no | `ok` + `message` + `qr` |
| [`watch`](#watch) | JSON | no (frames flow while connected) | `{"ok":true,"message":"watching"}`, then frames and local events (stream) |
| [`send`](#send) | JSON | yes, except service 25 `cap` updates | `ok` + `message` |
| [`reply`](#reply) | JSON | yes | `ok` + `message` |
| [`notif_mute`](#notif_mute) | JSON | no | `ok` + `muted` |
| [`sms_state`](#sms_state) | JSON | no | SMS snapshot |
| [`sms_threads`](#sms_threads) | JSON | yes | `ok` + `enabled` + `threads` |
| [`sms_thread`](#sms_thread) | JSON | yes | `ok` + `thread` + `messages` |
| [`sms_send`](#sms_send) | JSON | yes | `ok` + `message` + `requestId` + `to` |
| [`sms_mark_read`](#sms_mark_read) | JSON | yes | `ok` + `message` |

---

### `status`

Returns the current [status object](#the-status-object) as one line.

```
→ status
← {"connected":true,"paired":true,"name":"Phone","model":"moto g","battery":76,"charging":false,"wifi":true,"wifiSignal":4,"cellSignal":3,"bluetooth":true,"unread":2,"wallpaper":"/run/user/1000/smartconnectd/phone-wallpaper.jpg?<md5>","pairingQr":"/home/user/.local/share/smartconnect-linux/pairing-qr.svg","pcAddress":"192.168.1.20","phoneAddress":"192.168.1.30","notifications":[{"key":"0|com.example.chat|42|null|10150","service":5,"app":"Chat","pkg":"com.example.chat","title":"Alex","body":"See you at 6","replyable":true}]}
```

### `subscribe`

Streams the status. The first line is the current status object, and after that **every change
produces a complete status object** (not a diff). Changes come from phone status pushes, connection
changes, notifications being posted or removed, and pairing QRs. If the client falls more than 32
changes behind, the skipped ones are replaced by one fresh snapshot. The stream ends when the client
disconnects or the daemon stops.

```
→ subscribe
← {"connected":false,"paired":true,…}
← {"connected":true,"paired":true,…}
← {"connected":true,"paired":true,…,"battery":75,…}
```

### `connect`

Connects to the paired phone now. This is the same as the Connect buttons in the front-ends.

1. Only one connect runs at a time; a second request waits for the running one (for example the
   auto-reconnect loop's). If a session is up by then, the reply is
   `{"message":"already connected","ok":true}`.
2. A previous user Disconnect is cleared, so automatic reconnects resume.
3. For each phone address remembered from its mDNS advert, the daemon opens the phone's discovery
   endpoint and pushes a sealed invite. On success the reply is
   `{"message":"invite sent over Wi-Fi; the phone should connect in a moment","ok":true}`.
4. Otherwise, unless the daemon runs with `--no-ble-wake` and only while the PC's Bluetooth is
   already on, it scans Bluetooth LE for up to 30 s, connects to the phone (2 attempts of 12 s; if
   both fail it rescans once), writes the invite, and waits up to 8 s for the phone to dial in. The
   reply is then
   `{"message":"connected (woke the phone over Bluetooth)","ok":true}` or
   `{"message":"invite sent over Bluetooth; the phone should connect in a moment","ok":true}`.
5. On failure it returns `{"message":"<reason>","ok":false}`, where the reason starts with what the
   Wi-Fi attempt showed (`the phone's Smart Connect app is in the background` when the phone refused
   the dial, else `the phone isn't reachable over Wi-Fi`) followed by the Bluetooth reason, listed in
   [Troubleshooting](troubleshooting.md#the-phone-isnt-found-over-bluetooth). The daemon then keeps
   retrying in the background (the auto-reconnect loop).

Allow at least 90 s for the reply. `ok:true` means the invite was delivered, and the phone then
connects by itself. Watch `connected` in `status` or `subscribe` to see it happen.

### `disconnect`

Closes the current session and records a user Disconnect: the daemon won't reconnect on its own
until the next `connect`, or until the phone itself connects.

```
→ disconnect
← {"message":"disconnected","ok":true}        or  {"message":"not connected","ok":false}
```

### `pair`

Creates a fresh pairing invite. It renders the QR to the daemon's stdout (the journal under systemd)
and to the SVG file, and returns the SVG path. The token is valid for `--token-ttl` seconds. The
status object's `pairingQr` is updated too.

```
→ pair
← {"message":"scan this QR in Smart Connect","ok":true,"qr":"/home/user/.local/share/smartconnect-linux/pairing-qr.svg"}
```

The daemon rewrites the same path whenever it makes a new token, so reload the file instead of
caching the image.

### `watch`

Streams frames received from the phone: the raw service traffic the helpers are built on.

```
→ {"cmd":"watch","services":[15,16]}
← {"ok":true,"message":"watching"}
← {"data":{"cmd":3,"data":{"sessionId":123,"webRTCData":"…"},"ver":"1"},"service":15}
← …
```

| Request field | Type | Meaning |
|---|---|---|
| `services` | array of integers, optional | Only relay these service ids. Missing or empty means every phone service. List the reserved id `-1` to also get the daemon's [local events](#local-events-service--1); an empty list never includes them. |

- Each line is `{"data":<payload>,"service":<id>}`. `data` is the phone message's `data` payload,
  unmodified. The envelope (`msgId`, `srcServiceId`, `deviceId`) isn't included. Only text frames
  that carry `data` are relayed.
- Frames from every connected phone go to every watcher, in the order received. A watcher more than
  256 frames behind silently loses the oldest ones.
- The connection becomes stream-only (see [Framing](#framing)). The stream ends when the client
  disconnects or the daemon stops. Frames only arrive while a phone is connected, but the stream
  survives phone reconnects.
- Payloads are **not redacted** on this socket. Notification text, clipboard text and SMS pass
  through as the phone sent them.

Service ids you are likely to see (the payloads are described in the protocol pages):

| Service | What | Protocol page |
|---|---|---|
| 0 | Connection manager: hello, key exchange, `cmd 6` = the phone is disconnecting | [pairing and discovery](protocol/pairing-and-discovery.md) |
| 1 | Screen mirroring control | [mirroring](protocol/mirroring.md) |
| 3 | Webcam | [webcam](protocol/webcam.md) |
| 5, 37 | Notifications (posted, updated, removed) | [notifications](protocol/notifications.md) |
| 9 | Clipboard | [clipboard](protocol/clipboard.md) |
| 13 | Hotspot | [hotspot](protocol/hotspot.md) |
| 15 | Stream signalling (mirror and app streaming, WebRTC offer/answer/ICE) | [app streaming](protocol/app-streaming.md) |
| 16 | Remote input | [mirroring](protocol/mirroring.md) |
| 17 | Phone app list | [app streaming](protocol/app-streaming.md) |
| 22 | PC lock | [PC lock](protocol/pc-lock.md) |
| 25 | Device status (battery, signal, wallpaper, capabilities) | [device status and tiles](protocol/device-status-and-tiles.md) |
| 26 | Cross Control | [Cross Control](protocol/cross-control.md) |
| 100 | Unread message counts and Messages availability (`cmd 1018`) | [SMS](protocol/sms.md) |
| 202 | Phone as an external display | [phone as display](protocol/phone-as-display.md) |
| 204 | SMS | [SMS](protocol/sms.md) |
| -1 | Not a phone service: the daemon's [local events](#local-events-service--1) | |

#### Local events (service -1)

Service id `-1` carries events that the daemon itself produces for desktop helpers. They never go to
or come from the phone, and a watcher only receives them when it lists `-1` explicitly:

```
→ {"cmd":"watch","services":[15,-1]}
← {"ok":true,"message":"watching"}
← {"data":{"actionId":1,"event":"notification_action","key":"0|com.example.clock|7|null|10123","kind":"action","pkg":"com.example.clock","service":5},"service":-1}
```

There is one event so far, `notification_action`. It is sent just before a click on a phone
notification goes to the phone, both for clicks on the daemon's own desktop notifications and for
click messages sent through [`send`](#send) (service 5 or 37, `cmd 0` with `event` 1 or 2). On the
tested phone such a click makes the phone ask the PC to stream that app (service 15 `cmd 2`), and
the stream host uses this event to recognise the request and run action buttons without a window.
Replies (`event` 0) need no stream and produce no event.

| Field | Meaning |
|---|---|
| `event` | `"notification_action"`. |
| `kind` | `"action"` for an action button (phone event 1), `"open"` for a click on the notification itself (event 2). |
| `pkg` | The app's package, taken from the notification key. |
| `key` | The phone's notification key. |
| `actionId` | The button's `action_id`, or `null` for `"open"`. |
| `service` | 5 or 37, the service the click goes out on. |

### `send`

Forwards one message to the phone on the given service.

```
→ {"cmd":"send","service":16,"data":{"cmd":4,"data":{"displayId":0,"x":100,"y":200},"messageId":0,"instanceId":0}}
← {"message":"sent","ok":true}
```

| Request field | Type | Meaning |
|---|---|---|
| `service` | integer, required | Target service id. Also used as the envelope's `srcServiceId`. |
| `data` | any JSON, required | Becomes the envelope's `data` exactly as given. |

- The daemon wraps `data` in the phone's envelope (`deviceId` = this PC's per-run id, `serviceId`,
  `srcServiceId`, and a fresh `msgId`) and sends it on the current session. With `--session-log`,
  each message is also written to the [session log](security.md#session-logs).
- **Exception, service 25 with a `cap` object:** each device-status message replaces the phone's
  whole tile map, so forwarding a helper's partial map would hide every other tile. The daemon
  therefore doesn't forward it. It merges the `cap` entries into its own overrides, key by key (a
  later value for the same service replaces the earlier one), and if a phone is connected it sends
  the complete map: the daemon's own tiles, plus `--extra-tiles`, plus all overrides, together with
  the battery level and PC name. The reply is `{"message":"merged into the device status map","ok":true}`,
  even when no phone is connected. Other fields in `data` are ignored. Overrides stay in effect,
  including on later connections, until a helper sends a new value for that service or the daemon
  restarts. Cross Control uses this to flag its tile as in use (`{"26":9}`) and to clear the flag
  again (`{"26":1}`); if it crashes in between, the flag stays until the daemon restarts.
- **Notification clicks** (service 5 or 37, `cmd 0` with `event` 1 or 2) also produce a
  [local event](#local-events-service--1) before they are sent.
- **No validation.** The phone receives exactly what you send. Some messages have lasting effects:
  clipboard `msgId` 101, for instance, disables the clipboard for this PC on the phone persistently.
  Read the [protocol pages](protocol/README.md) first, and copy payload shapes from the helpers'
  source.
- Replies from the phone arrive on a [`watch`](#watch) connection. Open it **before** sending, so
  the answer isn't missed.
- Messages queued faster than the session can send them (more than 32 waiting) can be dropped.

### `reply`

Replies to a phone notification through the app's own reply action on the phone.

```
→ {"cmd":"reply","key":"0|com.example.chat|42|null|10150","text":"See you at 6"}
← {"message":"reply sent","ok":true}
```

| Request field | Type | Meaning |
|---|---|---|
| `key` | string, required | A notification `key` from the status object's `notifications`. |
| `text` | string, required, not blank | The reply text. Line breaks are allowed. |

The daemon sends the reply on the service the notification came from (5 or 37, defaulting to 5 for
unknown keys). Only notifications with `"replyable":true` can be answered.

### `notif_mute`

Lists, adds or removes apps whose notifications don't pop up on the desktop. They still appear in
the status object's `notifications`.

```
→ {"cmd":"notif_mute"}                                          list
← {"muted":["com.example.game"],"ok":true}
→ {"cmd":"notif_mute","pkg":"com.example.chat","mute":true}     mute (mute defaults to true)
← {"muted":["com.example.chat","com.example.game"],"ok":true}
→ {"cmd":"notif_mute","pkg":"com.example.chat","mute":false}    unmute
← {"muted":["com.example.game"],"ok":true}
```

| Request field | Type | Meaning |
|---|---|---|
| `pkg` | string, optional | Android package name. Without it the command only lists. |
| `mute` | boolean, optional, default `true` | `false` unmutes. |

The list is saved to `~/.local/share/smartconnect-linux/notif-mute.json` (always under
`$XDG_DATA_HOME`, even with `--state-dir`) and takes effect immediately. If saving fails, the reply
is `{"message":"saving <path>: <error>","ok":false}`. The in-memory list has already changed by
then.

Every successful reply carries the whole list in `muted`. The GTK app and the Noctalia plugin keep
no list of their own: they show the latest `muted` they received, and take the package to mute from
each notification's `pkg` in the [status object](#the-status-object).

### SMS commands

The phone's Messages service (service 204). Requests to the phone are fire-and-forget: each command
returns what the daemon **has cached right now**, and the phone's answers fill the cache a moment
later. Poll `sms_state` or repeat the command to see new data. The cache is kept in memory only.
Status 🟡: implemented, untested. See [Usage](usage.md#sms).

#### `sms_state`

The whole cache. Works without a connected phone. This reply has no `ok` field.

```
→ {"cmd":"sms_state"}
← {"enabled":true,"message_count":120,"send_availability":{…},"send_results":[…],"sims":{…},"threads":[…],"usable":{…}}
```

| Field | Meaning |
|---|---|
| `usable` | The phone's latest Messages availability object as it sent it, for example `{"enabled":true,"granted":true,…}`, or `null`. |
| `enabled` | `usable.enabled`: whether the phone answers Messages requests from this PC. |
| `message_count` | Number of cached messages (SMS and MMS). |
| `threads` | [Thread objects](#sms-objects), newest first. |
| `sims` | The phone's own number and SIM info as it sent it, or `null`. |
| `send_availability` | The phone's "can send" answer as it sent it, or `null`. |
| `send_results` | Up to 20 recent send acknowledgements, newest first: `{"cmd":5,"data":{"requestId":…}}` (accepted) and `{"cmd":6,"data":{"requestId":…,"status":…,…}}` (result). |

#### `sms_threads`

Starts a sync and returns the cached conversations. The sync asks for the phone number, contacts,
and all SMS from about the last 10 days. If Messages isn't enabled for this PC yet, the daemon first
sends the "PC ready" message that enables it, and syncs again once the phone reports `enabled:true`.

```
→ {"cmd":"sms_threads"}
← {"enabled":true,"ok":true,"threads":[{"address":"+15555550123","count":12,"last_body":"See you at 6","last_date":1761000000000,"name":"Alex","thread":"5555550123","unread":1}]}
```

#### `sms_thread`

Returns one conversation's cached messages, oldest first, and asks the phone to re-send its SMS list.

```
→ {"cmd":"sms_thread","thread":"5555550123"}
← {"messages":[{"id":1201,"kind":1,"message_type":1,"from":"+15555550123","to":["<own number>"],"body":"See you at 6","date":1761000000000,"read":false,"sub_id":1}],"ok":true,"thread":"5555550123"}
```

| Request field | Type | Meaning |
|---|---|---|
| `thread` or `number` | string, one required | A thread key from `sms_threads`, or a phone number or sender name. Normalised to a thread key; the reply's `thread` shows the result. |

#### `sms_send`

Sends an SMS from the phone's default SIM.

```
→ {"cmd":"sms_send","number":"+15555550123","text":"On my way"}
← {"message":"sms handed to phone","ok":true,"requestId":42300001,"to":"+15555550123"}
```

| Request field | Type | Meaning |
|---|---|---|
| `number` | string | Recipient. Takes precedence over `thread`. |
| `thread` | string | Alternatively, a thread key. The message goes to that conversation's counterpart. |
| `text` | string, required, not blank | Message body. |

The outcome arrives later in `sms_state.send_results` under the same `requestId`.

#### `sms_mark_read`

Marks one SMS or MMS as read on the phone.

```
→ {"cmd":"sms_mark_read","id":1201}
← {"message":"sent","ok":true}
```

| Request field | Type | Meaning |
|---|---|---|
| `id` | integer, required | A message `id` from `sms_thread`. |
| `kind` | integer, optional | 1 for SMS (default), 2 for MMS. Other values fail validation. |

## The status object

Returned by `status` and streamed by `subscribe`. Fields keep this order. Optional values are `null`
when unknown.

| Field | Type | Meaning |
|---|---|---|
| `connected` | bool | A phone session is up. |
| `paired` | bool | At least one phone is paired with this PC. |
| `name` | string or null | Phone name, from its hello or status. |
| `model` | string or null | Phone model, from its status. |
| `battery` | int or null | Battery percentage. |
| `charging` | bool | |
| `wifi` | bool | The phone's Wi-Fi is on. |
| `wifiSignal` | int or null | Wi-Fi signal level as the phone reports it (the front-ends map 0–5 to None … Excellent). |
| `cellSignal` | int or null | Mobile signal level, same scale. |
| `bluetooth` | bool | The phone's Bluetooth is on. |
| `unread` | int or null | Unread SMS/MMS count reported by the phone (service 100 cmd 1018). Independent of `notifications`; do not use as an app-notification badge. |
| `wallpaper` | string or null | Path of the phone's wallpaper image, with `?<md5>` appended so image widgets reload when it changes. Strip the suffix to open the file. |
| `pairingQr` | string or null | Path of the current pairing QR (SVG). Rewritten in place with every new token. |
| `pcAddress` | string or null | This PC's address on the phone's network: the local address of the last verified phone connection. `null` until the first one; kept after a disconnect. The stream host and the phone display offer the phone WebRTC candidates on this address only (unless `SMARTCONNECT_ICE_ADDRESSES` is set). |
| `phoneAddress` | string or null | The phone's IP address on the last verified connection (the WebSocket peer). `null` until the first one; kept after a disconnect. `smartconnect-fex fetch-app-icons` pulls the app icons from this address. |
| `notifications` | array | Up to 30 recent phone notifications, newest first (see below). Kept in memory only. |

Each entry of `notifications`:

| Field | Type | Meaning |
|---|---|---|
| `key` | string | The phone's notification key. Pass it to [`reply`](#reply). |
| `service` | int | 5 or 37, the service it arrived on. |
| `app` | string | App name. |
| `pkg` | string | Android package name. Pass it to [`notif_mute`](#notif_mute). |
| `title` | string | Conversation title or notification title. |
| `body` | string | Readable body. Chat notifications show the last messages, one per line, with senders in group chats. |
| `replyable` | bool | The app accepts free-text replies. |

Notifications from muted apps are included. Custom-layout notifications aren't.

## SMS objects

Thread (in `threads`):

| Field | Meaning |
|---|---|
| `thread` | Thread key. Phone numbers are reduced to their last 10 digits, so local and international forms match; alphanumeric senders are lowercased. |
| `address` | The other party's address, as in the newest message. |
| `name` | Contact name if the phone sent one, else `null`. |
| `count` | Number of cached messages. |
| `unread` | Unread received messages. |
| `last_body`, `last_date` | Newest message text and time (Unix ms). |

Message (in `messages`):

| Field | Meaning |
|---|---|
| `id` | Phone message id (use it with `sms_mark_read`). |
| `kind` | 1 = SMS, 2 = MMS. |
| `message_type` | 1 = received, 2 = sent. |
| `from`, `to` | Sender address; recipient addresses. |
| `body` | Text. |
| `date` | Unix ms. |
| `read` | bool. |
| `sub_id` | SIM subscription id (-1 = default). |

## Error messages

| `message` | Commands | Cause |
|---|---|---|
| `unknown command "<x>"` | any | Unknown word, unknown `cmd`, or a JSON line without `cmd` (`<x>` is empty then). |
| `phone not connected` | `send`, `reply`, `sms_threads`, `sms_thread`, `sms_send`, `sms_mark_read` | No phone session. |
| `not connected` | `disconnect` | No phone session. |
| `send needs service and data` | `send` | `service` missing or not an integer, or `data` missing. |
| `empty reply` | `reply` | `key` missing or `text` blank. |
| `saving <path>: <error>` | `notif_mute` | The mute list couldn't be written. |
| `sms_thread needs thread` | `sms_thread` | Neither `thread` nor `number` given. |
| `sms_send needs number (or known thread) and text` | `sms_send` | No recipient, an unknown thread, or blank text. |
| `sms_mark_read needs id` | `sms_mark_read` | `id` missing or not an integer. |
| *(error chain)* | `connect`, `pair` | Connect or QR rendering failed. The text explains why. |

## The stream host socket

`smartconnect-mirror --host` serves a second socket for front-ends that manage streams (the GTK
app's *Apps* page and Mirror button use it).

| | |
|---|---|
| Path | `$XDG_RUNTIME_DIR/smartconnect-mirror.sock`. It exists only while a host runs, and a second host refuses to start while it answers. |
| Framing | One JSON request per line, one JSON reply per line, several requests per connection. Plain words are not accepted. |
| Timeout | If the host doesn't handle a request within 5 s, the reply is `{"ok":false}`. |

| Request | Reply | Effect |
|---|---|---|
| `{"cmd":"app","pkg":"<package>","cmp":"<component>"}` | `{"message":"requested","ok":true}` | Queues a PC-initiated stream of that app (`cmp` is optional). If the app already has a window, it is raised first (niri only). Missing `pkg` gives `{"message":"app needs pkg","ok":false}`. |
| `{"cmd":"mirror"}` | `{"message":"requested","ok":true}` | Asks the phone for a whole-screen mirror. |
| `{"cmd":"desktop"}` | `{"message":"requested","ok":true}` | Asks the phone for Ready For's Mobile desktop view (experimental, not verified on a device). |
| `{"cmd":"desktop","width":1920,"height":1080}` | `{"message":"requested","ok":true}` | Sets the phone desktop resolution through service 1 command 33, then requests the desktop with that service-15 size bound. Both dimensions must be integers, even, landscape, width 640–3840 and height 360–2160. Invalid sizes fail before sending anything. Close a running desktop before changing its size. The phone may limit the resolution. |
| `{"cmd":"capabilities"}` | `{"ok":true,"desktopResolution":true,"desktopBorderless":true}` | Use before sending dimensions or window styles: older hosts ignore them. `desktopBorderless` is false for unsupported renderers. |
| `{"cmd":"desktop_window","borderless":true}` | `{"ok":true}` | Toggle decorations on existing desktop windows without changing phone streams. Validates a boolean and rejects enabling the mode on unsupported renderers. Other app windows are unchanged. |
| `{"cmd":"sessions"}` | `{"ok":true,"sessions":[{"displayId":67,"headless":false,"pkg":"com.example.calculator","sessionId":123,"title":"Calculator — Phone"}]}` | Lists open streams. `pkg` is `"mirror"` for the whole-screen mirror, and `"desktop"` or `"mobile"` for the Mobile desktop and virtual-phone views. `displayId` is `null` until the phone reports it. `headless` is `true` for the windowless sessions that run notification action buttons; they last a few seconds, and a UI listing windows should skip them. |
| `{"cmd":"close","sessionId":123}` | `{"ok":true}` or `{"message":"no such session","ok":false}` | Closes that window and tells the phone to end the stream. |
| `{"cmd":"apps"}` | `{"apps":[{"cmp":"…","iconName":"…","name":"…","pkg":"…","userId":0,"work":false}],"ok":true}` | The host's cached app list (`work` marks work-profile apps; `iconName` is the icon's file name in `~/.cache/smartconnect-linux/app-icons/`). Run `smartconnect-mirror --list-apps` to refresh it from the phone. |
| anything else | `{"message":"commands: app {pkg,cmp?}, sessions, close {sessionId}, apps, mirror, desktop","ok":false}` | |
| invalid JSON | `{"message":"<parse error>","ok":false}` | |

Hosts advertise `displayRotation:true` when they handle native phone rotation events. Session
entries expose the last reported `rotation` in quarter turns (0–3). Native app and virtual-phone
windows rotate their picture and input mapping; fixed layouts retain their window geometry.
Complete desktops and scrcpy retain their own orientation handling.

`app` accepts optional `width`, `height` and `density` for an exact virtual-display request,
e.g. `{"cmd":"app","pkg":"com.example.notes","width":720,"height":1280,"density":240}`.
Hosts advertise `desktopScrcpy:true`, `appScrcpy:true` and `scrcpyFullscreen:true`. Both `app` and `desktop` optionally accept `renderer:"builtin"` (default)
or `renderer:"scrcpy"`, with `scrcpy:{adbDevice:"",adbPort:5555,softwareDecode:false}`. The ADB
selector is bounded to 255 ASCII letters/digits or `._-:%[]`; ports are 1–65535. Invalid selected
scrcpy settings fail before phone commands. A backend/options change requires stopping the old
app/desktop, like geometry/quality changes. `desktop_window` cannot change a running scrcpy window's
decorations; restart instead.

`desktop` additionally accepts `fullscreen:true` (boolean; default false), forwarded to scrcpy only. The GUI requests it when following the PC resolution. Sessions report `fullscreen`. App height/width fit passes matching native window dimensions and applies X11 aspect/size hints to the owned child; ordinary app windows are not fullscreen.

Starting is asynchronous. Session entries add `renderer`, `requestedScrcpy`, `scrcpyState`
(`preparing`, `starting`, `running`; null for built-in), and `scrcpyPid` (null before spawn).
The response adds `desktopPending` while the phone creates the desktop and optional
`desktopError` after failed ADB/scrcpy startup. Failure removes the source session; a new desktop
request clears the error. App failures are reported separately in root `appErrors:{"com.example.notes":"<error>"}`; retrying that app clears its old error. They do not overwrite `desktopError` or stop unrelated sessions. Closing the child also closes the source and vice versa. PID refers
only to the host-owned child. The API does not accept arbitrary scrcpy command-line arguments.

Hosts advertise `streamQuality:true` for optional `quality:{bitrateKbps:2500,fps:30}` on
`app` and `desktop`. Omitted/null members mean automatic. Bitrate is 300–50000 kbit/s; FPS
is 1–60. Invalid values fail before phone requests. Sessions expose `requestedQuality` and
`displayRateDropped` (frames skipped by the decoded-frame limiter, distinct from renderer drops).
A different profile for an open app/desktop requires closing it first. The bitrate ceiling is
negotiated through video SDP; FPS is also limited at the receiver, with no encoder-rate guarantee.

`appLayout:true` means the default XWayland window path supports optional app
`layout:{fixed:true,ratioWidth:9,ratioHeight:16,fit:"height"}`. `fit` is `manual`, `height` or
`width`. A fixed layout requires a valid custom app resolution matching the ratio within
rounding tolerance. Compute physical-pixel width/height before sending; the GUI handles this
from the target monitor. Height/width fit requests borderless native-sized client geometry;
manual fixed ratio only sets aspect hints. Oversized source dimensions fail validation.
Sessions expose `requestedLayout`; headless actions and view sessions do not inherit app layout.

Hosts advertise `appResolution:true`. Width/height must both be present, even and 320–3840;
the shorter side must be at most 2160. Density defaults to 320 and must be 120–640; density
alone is rejected. Omitting all three retains automatic sizing. Invalid/view-session requests
fail before any phone command. An existing app window must be closed before changing its
profile; a pending launch with another profile is rejected. Profiles travel with individual
queued requests and do not change other apps or the desktop.

For custom app sessions, `requestedWidth`, `requestedHeight` and `requestedDensity` expose the
requested values, while `width`, `height` and `density` retain the phone's actual report. Null
requested values mean automatic sizing. The phone/app can impose additional limits.

`desktop` additionally accepts `scale`, an integer percentage from 100 to 300 (omitted: 100).
Hosts advertise `desktopScaling:true`. A changed scale needs the desktop session closed first;
GUI preferences offer restart-to-apply. `requestedScale` reports the desktop request and `density`
is the phone-reported DPI, or null before known. Scaling changes virtual-display density and
physical dimensions, preserving the encoded width/height; standalone app streams retain their
existing behavior. The GTK default resolves the scale from the monitor containing its window.

The `desktop` command also accepts an optional boolean `borderless`, applied to new or existing
desktop sessions. Session entries expose their current `borderless` flag.

Session entries include `udpReceiveBuffers`, an array of effective `SO_RCVBUF` byte values
for libnice UDP sockets discovered on that session. The viewer requests 4 MiB per socket;
Linux reports a doubled value for kernel accounting and caps the request at `net.core.rmem_max`.
An empty array means no sockets have been discovered yet. No system sysctls are changed.

Session entries also include `videoProgress`: cumulative `rtpPackets`, `decodedFrames`,
`inputEvents` and recovery `keyframeRequests`, plus nullable `lastPacketAgeMs`,
`lastDecodedAgeMs` and `lastInputAgeMs`. Counters contain no video or input contents;
input counts include button presses, scrolls and key presses, excluding pointer motion.
Compare snapshots: increasing packets without decoded frames suggests a receive/decode stall;
increasing decoded frames without renderer progress suggests a rendering stall. No new
packets on an unchanged phone screen can be normal. Input transmission is not an acknowledgement
that the phone acted on it.

Sessions also expose `streamTiming` (received/released packet totals, sender RTP timestamp
regressions, maximum arrival/RTP interval difference, maximum jitterbuffer hold and packets
held over 100 ms) and `jitterStats` (per-buffer pushed/lost/late/duplicate packets,
`avgJitterNs` and retransmission requests). Counters/maxima cover the session lifetime;
queue residence is measured locally, not end-to-end latency. Static displays may stop sending.

Session entries include optional `renderStats:{rendered,dropped}` cumulative sink frame counters
(or null before a renderer exists or with a sink that does not expose statistics).

Session entries also include `width`/`height` (the phone's reported stream dimensions, or null
until known). For `pkg:"desktop"`, `requestedWidth`/`requestedHeight` report the requested desktop bounds
when available. These fields describe the media size; they do not measure window size or
confirm that every frame was presented. Unknown fields can be ignored by older clients.

The host starts streams one at a time. It waits up to 20 s for the phone to answer each start, so
`requested` doesn't mean the window is open yet. Poll `sessions` to see it appear. A request for an
app that is already queued or being started is ignored.

## Compatibility

The API isn't versioned yet and may change between releases. To stay compatible:

- ignore fields you don't know (the GTK app gives every status field a default value);
- don't depend on key order or on exact `message` texts, except for checking `ok`;
- reconnect when the socket closes, because the daemon may have restarted.

### Cancelling queued and preparing mirror starts

`capabilities` advertises `cancelStart:true`. Send `{"cmd":"cancel","pkg":"desktop"}`, `"mirror"`, or an app package. This is idempotent: queued/in-flight starts are removed, matching visible sessions are closed, and owned scrcpy/helper processes are cancelled asynchronously. App cancellation leaves other apps running. View cancellation also sends the existing service-1 stop command; view modes remain mutually exclusive.

`sessions` adds `pending:["com.example.notes","desktop"]` (sorted unique packages) and `mirrorPending`, alongside `desktopPending`. Session entries cover the later scrcpy preparing/starting phases. Late invites/offers for a cancelled package are discarded for 30 seconds; a late known session ID receives service-15 close without an SDP answer. An explicit app/desktop/mirror request clears its cancellation marker so a user can retry. GUI cancellation serializes with the request worker to prevent cancellation during host startup from racing a delayed start.

### Optional ADB preparation before starting

Hosts advertise `adbWakeUnlock:true`. Both `app` and `desktop` accept `scrcpy:{wakeUnlock:true,adbDevice:"",adbPort:5555}`, even with `renderer:"builtin"`; omitted `wakeUnlock` defaults to false. ADB settings are validated before preparing. No scrcpy executable is needed for this preparation when using the built-in renderer.

The request is acknowledged asynchronously. `sessions.pending` includes packages waiting on ADB and `adbPreparing:["com.example.notes"]` identifies that stage. No Smart Connect app/desktop start command is sent before wake/dismiss and unlocked-state verification succeed. Failure appears in `appErrors` or `desktopError` and removes the pending start; `cancel {pkg}` also cancels the owned ADB process and prevents a delayed continuation. Session `requestedScrcpy` retains the selected `wakeUnlock` value. Secure-keyguard/unknown-policy results stop preparation and ask for manual authentication.

Hosts advertise `foldMouse:true` for `scrcpy.foldMouse` (boolean, default true) on app/desktop commands and `requestedScrcpy`. Enabled selects a source-bound UHID mouse only for a virtual source, Android SDK 35+ and scrcpy major 4+; otherwise the host retains SDK mouse input. False always requests SDK mouse input. Keyboard mode is selected independently through `phoneIme`. The choice applies on a new projection, never changes a running child, and does not add new Smart Connect phone protocol fields.

### Cache-only Messages reads

`sms_threads` and `sms_thread` accept `refresh:false` to read RAM without issuing a phone query. Default/true retains explicit sync behavior. Responses advertise `cacheOnlySupported:true`; clients should check this before repeated cache reads, since older daemons ignore unknown fields. The GTK Messages page requests one sync on opening/manual refresh, then performs at most eight cache-only reads over two seconds to collect asynchronous replies. It does not schedule recurring phone polling. Opening an unread message can mark it read with its actual kind. A successful `sms_send` reply acknowledges handoff only; delivery remains in `sms_state.send_results`.

Hosts advertise `scrcpyInputSettings:true`. `scrcpy.mouseSpeedPercent` is an integer from 25 through 400 (default 100). It sets the child-only `SDL_MOUSE_RELATIVE_SPEED_SCALE` when the effective mouse mode is UHID; SDK mouse input is unchanged. `scrcpy.phoneIme` is a boolean (default true), selecting a source-bound UHID keyboard on Android SDK 35+/scrcpy 4+ with a virtual source. False or unsupported/unknown environments retain SDK keyboard injection. Both fields are returned in `requestedScrcpy` and apply when creating a new projection. For example: `{"cmd":"desktop","renderer":"scrcpy","scrcpy":{"foldMouse":true,"mouseSpeedPercent":150,"phoneIme":true}}`.

`capabilities.computerIme` reports whether the optional SDL3 adapter was built. `scrcpy.computerIme` is a boolean, default false, returned in `requestedScrcpy`. True takes precedence over the saved `phoneIme` flag and selects SDK keyboard plus `--prefer-text`; only confirmed SDL text commits use Smart Connect service 16 command 7 for the owning session's current display. This requires scrcpy 5+; unsupported environments return a startup error. There is no new public text-injection socket command.

Hosts advertise `scrcpyAudio:true`. `scrcpy.audio` is a boolean (default true), persisted independently for app/desktop and exposed in `requestedScrcpy`. True passes `--audio-source=output`; false passes `--no-audio`. It never selects a microphone source or requires audio to keep video running. The selection applies on the next projection, without changing the built-in renderer.

`capabilities.independentAudio:true` supports boolean `scrcpy.independentAudio` (default false), persisted and exposed in `requestedScrcpy`. With `audio:true`, it selects `--audio-source=playback` and requires a verified Android SDK level >=33; unsupported/unknown levels return a startup error. With `audio:false`, `--no-audio` wins and the saved independent setting is ignored. No phone volume setter or duplication flag is sent.
