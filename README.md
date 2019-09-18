# RQueue

A prioritized notifications queing proxy.

If not otherwise specified, notifications have a priority of 10. Higher priority notifications will be delivered before lower priority notifications.

## Structure

### Message in

The value of `contents` must be set when POSTing to the proxy, and contains any string.
The value of `priority` can be set to an unsigned integer value from 0 to 255, or it will automatically be set to 10.

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

Started with the Rocket JSON example:
<https://github.com/SergioBenitez/Rocket/tree/v0.4/examples/json>

Reworked to implement a prioritized queue for proxying notifications. As it's built on Rocket,
it requires Rust nightly:

```bash
rustup default nightly
```

Tests can be run as follows:

```bash
cargo test
```

Alternatively, the daemon can be tested manually with curl. First, run the daemon:

```bash
cargo run
```

Now, for example, add an element to the queue:

```bash
curl -X POST http://localhost:8000/ -H 'Content-type: application/json' --data '{"contents": "one"}'
{"code":202,"status":"accepted"}
```

Add another element:

```bash
curl -X POST http://localhost:8000/ -H 'Content-type: application/json' --data '{"contents": "two"}'
{"code":202,"status":"accepted"}
```

Get the first element (a default priority of 10 was auto-assigned):

```bash
curl -X GET http://localhost:8000/
{"contents":"one","priority":10,"elapsed":31159}
```

Get the second element (when not using priorities, it's a FIFO queue):

```bash
curl -X GET http://localhost:8000/
{"contents":"two","priority":10,"elapsed":26241}
```

There's nothing else in the queue:

```bash
curl -X GET http://localhost:8000/
{"code":404,"reason":"Empty queue.","status":"error"}
```

A higher priority element can be added to the queue using the `priority` parameter:

```bash
curl -X POST http://localhost:8000/ -H 'Content-type: application/json' --data '{"contents": "three", "priority": 50}'
{"code":202,"status":"accepted"}
```

## TODO

* Track daemon uptime (and optionally expose as debug)
* Track response timing (and expose as debug/log)
* Provide better error handling for invalid priority
* Optionally require configurable JWT authentication to post to queue
* Implement method for pushing queued data via HTTP/S
* Implement disk-backing for items in queue over configurable amount of time
* Add configuration for enabling/disabling debug output
* Add configurable logging
* Add configuration to cap memory usage (and track memory usage)
* Add configuration to cap storage usage (and track storage useage) for disk-backing
