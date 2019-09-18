# RQueue

A prioritized notifications queing proxy.

If not otherwise specified, notifications have a priority of 10. Higher priority notifications will be delivered before lower priority notifications.

## Structure

### Message in

The value of `contents` must be set when POSTing to the proxy, and contains any string.
The value of `priority` can be set to an integer value from 0 to 99, or it will automatically be set to 10.

```json
{
    "contents": "String",
    "priority": 10,
}
```

### Message out

Includes the above, also adding:

The value of `elapsed` indicates how many milliseconds the message was in the queue.

```json
{
    "contents": "String",
    "priority": 10,
    "elapsed": 325,
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
{"code":202,"status":"accepted"}
```

Add another element:

```bash
curl -X POST http://localhost:8000/ -H 'Content-type: application/json' --data '{"contents": "two"}'
{"code":202,"status":"accepted"}
```

Get the first element:

```bash
curl -X GET http://localhost:8000/
{"contents":"one","priority":10,"elapsed":31159}
```

Get the second element:

```bash
curl -X GET http://localhost:8000/
{"contents":"two","priority":10,"elapsed":26241}
```

There's nothing else in the queue:

```bash
curl -X GET http://localhost:8000/
{"code":404,"reason":"Resource was not found.","status":"error"}
```

Add a higher priority element to the queue:

```bash
curl -X POST http://localhost:8000/ -H 'Content-type: application/json' --data '{"contents": "three", "priority": 50}'
{"code":202,"status":"accepted"}
```
