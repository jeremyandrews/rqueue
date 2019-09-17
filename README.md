# RProxy

A prioritized notifications proxy.

If not otherwise specified, notifications have a priority of 10. Higher priority notifications will be delivered before lower priority notifications.

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
