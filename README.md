# RQueue

A prioritized notifications queing proxy.

If not otherwise specified, notifications have a priority of 10. Higher priority notifications will be delivered before lower priority notifications.

## Structure

### Message in

The value of `contents` must be set when POSTing to the proxy, and contains any string.
The value of `priority` can be set to an integer value from 0 to 99, or it will automatically be set to 10.

```json
{
    contents: String,
    priority: Number,
}
```

### Message out

Includes the above, also adding:

The value of `proxyArrive` indicates when the proxy received the notification (as an integer unix timestamp).
The value of `proxyDepart` and indicates when the proxy forwarded the notification (as an integer unix timestamp).

```json
{
    contents: String,
    priority: Number,
    proxyArrive: Number,
    proxyDepart: Number,
}
```

## Notes

Converted the Rocket JSON example to use a prioritized queue:
<https://github.com/SergioBenitez/Rocket/tree/v0.4/examples/json>

Built on Rocket, which requires Rust nightly:

```bash
rustup default nightly
```

Includes tests, which can be run the standard way:

```bash
cargo test
```

It's also possible to test manually with curl, first run the daemon:

```bash
cargo run
```

Add an element:

```bash
curl -X POST http://localhost:8000/ -H 'Content-type: application/json' --data '{"contents": "one"}'
{"status":"ok"}
```

Add another element:

```bash
curl -X POST http://localhost:8000/ -H 'Content-type: application/json' --data '{"contents": "two"}'
{"status":"ok"}
```

Get the first element:

```bash
curl -X GET http://localhost:8000/
{"contents":"one"}
```

Get the second element:

```bash
curl -X GET http://localhost:8000/
{"contents":"two"}
```

There's nothing else in the queue:

```bash
curl -X GET http://localhost:8000/
{"reason":"Resource was not found.","status":"error"}
```

Add a higher priority element to the queue:

```bash
curl -X POST http://localhost:8000/?priority=50 -H 'Content-type: application/json' --data '{"contents": "three"}'
{"status":"ok"}
```
