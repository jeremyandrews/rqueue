# RQueue

Prioritized proxy and notifications queue.

If not otherwise specified, notifications have a priority of 10. Higher priority notifications will be delivered before lower priority notifications.

## Features

### rqueue-proxy (default)

When enabled, rqueue operates as a prioritized proxying queue for notifications.

For example:

```bash
cargo build
```

### rqueue-notify

When enabled, rqueue operates as a prioritized notification server.

For example:

```bash
cargo build --no-default-features --features rqueue-notify
```

### rqueue-debug

When enabled, rqueue displays additional debug information through the REST endpoints.

For example:

```bash
cargo build --features rqueue-debug
```

## Structure

### Message in

* `contents` must be set, and can contain any string. It is generally assumed this will be an encrypted blob.
* `priority` is optional, and can contain an unsigned integer value from 0 to 255; if empty it will automatically be set to 10.
* `sha256` is optional by default, and if set must be the sha256 of `contents` (optionally salted with `shared_secret`)

#### Security

RQueue can be configured with a `shared_secret` which will get appended to the `contents` before hashing. If you also set `require_sha256 = true`, this provides a layer of security as items will not be added to the queue without a proper hash. For example, if your contents are `test` and your `shared_secret` is `foo`, then the required sha256 will be of `testfoo` instead of just `test`. If `sha256` is set to `sha256(test)` alone, it will not be accepted.

```json
{
    "contents": "String",
    "sha256": "b2ef230e7f4f315a28cdcc863028da31f7110f3209feb76e76fed0f37b3d8580",
    "priority": 10,
}
```

If `sha256` is set to someting other than the sha256() of `contents` (or `contentsfoo` if using a `shared_secret`), the message is not accepted.

### Message out

The following fields are added by the queue:

* `elapsed` indicates how many milliseconds the message was held in the queue
* `uuid` is an auto-generated unique identifier assigned to each item in the queue

Resulting in the following structure:

```json
{
    "uuid": "String",
    "sha256": "b2ef230e7f4f315a28cdcc863028da31f7110f3209feb76e76fed0f37b3d8580",
    "priority": 10,
    "contents": "String",
    "elapsed": 325,
}
```

## Configuration

All configuration is done through `Rocket.toml`, as documented here:
    https://rocket.rs/v0.4/guide/configuration/

Currently the only configuration specific to RQueue is to control the maximum size of
the queue in memory. By default it will be limited to 64 MiB, but an alternative limit
can be configured by defining `queue_memory_limit_in_bytes` in `Rocket.toml`. This limit
can be configured per-environment, or in the global section.

For example, to set the limit to 256 MiB in all environments:

```toml
[global]
queue_memory_limit_in_bytes = 268435456
```

Or, to set the limit to 8 MiB in development and 1 GiB in production (and it will default to 64 MiB in staging):

```toml
[development]
queue_memory_limit_in_bytes = 8388608

[production]
queue_memory_limit_in_bytes = 1073741824
```

Whatever value you set will be visible in Rocket's logs, for example:

```bash
    => [extra] queue_memory_limit_in_bytes: 1073741824
    => Queue memory limit: 1.00 GiB
```

## Notes

Rocket requires the nightly version of Rust:

```bash
rustup default nightly
```

Tests can be run as follows:

```bash
cargo test
```

Alternatively, the daemon can be tested manually. First, run the daemon:

```bash
cargo run
```

Now, for example, add an object to the queue with curl:

```bash
curl -X POST http://localhost:8000/ -H 'Content-type: application/json' --data '{"contents": "one"}'
{
    "code": 202,
    "debug": {
        "in_queue": 1,
        "process_time": 1,
        "proxied": 0,
        "proxy_requests": 0,
        "queue_requests": 1,
        "queue_size": "163 bytes",
        "queued": 1,
        "request_size": "163 bytes"
        "uptime": 13587
    },
    "status": "accepted"
}
```

The contents of the debug array will only be visible when the daemon is running in debug mode. They have the following meanings:

* `in_queue` indicates how many items are currently queued
* `process_time` indicates how many milliseconds it took to process your PUT
* `proxied` indicates how many items have been added to then read from the queue
* `proxy_requests` indicates how many times a request has been made to retreive something from the queue
* `queue_requests` indicates how many times a request has been made to store something in the queue
* `queue_size` indicates how much data is in the queue
* `queued` indicates how many times an item has been successfully stored in the queue
* `request_size` indicates how much data it took to store this request
* `uptime` indicates how many milliseconds the rqueue daemon has been running

Now, we can add another object:

```bash
curl -X POST http://localhost:8000/ -H 'Content-type: application/json' --data '{"contents": "two"}'
{
    "code": 202,
    "debug": {
        "in_queue": 2,
        "process_time": 1,
        "proxied": 0,
        "proxy_requests": 0,
        "queue_requests": 2,
        "queue_size": "326 bytes",
        "queued": 2,
        "request_size": "163 bytes"
        "uptime": 46589
    },
    "status": "accepted"
}
```

Now, we can grab the first object from the queue. It has been auto-assigned a priority of 10 becauase we didn't assign a different priority:

```bash
curl -X GET http://localhost:8000/
{
    "code": 200,
    "data": {
        "contents": "one",
        "elapsed": 71056,
        "priority": 10,
        "sha256": "7692c3ad3540bb803c020b3aee66cd8887123234ea0c6e7143c0add73ff431ed",
        "uuid": "ac782fc0-1fae-42e5-83a3-790e9c63a122"
    },
    "debug": {
        "in_queue": 1,
        "process_time": 0,
        "proxied": 1,
        "proxy_requests": 1,
        "queue_requests": 2,
        "queue_size": "163 bytes",
        "queued": 2,
        "uptime": 84643
    },
    "status":"ok"
}
```

The debug array contains the same information displayed for POSTs. The data array includes some new information:

* `priority` was auto-assigned to 10, as no value was manually assigned when the data was POSTed
* `sha256` was auto-assigned to a SHA256 hash of the contents string ("one")
* `uuid` was auto-assigned to a random version 4 UUID, uniquely identifying this specific contents

A higher priority object can be added to the queue by including the `priority` parameter:

```bash
curl -X POST http://localhost:8000/ -H 'Content-type: application/json' --data '{"contents": "three", "priority": 50}'
{
    "code": 202,
    "debug": {
        "in_queue": 2,
        "process_time": 0,
        "proxied": 1,
        "proxy_requests": 1,
        "queue_requests": 3,
        "queue_size": "328 bytes",
        "queued": 3,
        "request_size": "165 bytes",
        "uptime": 179334
    },
    "status": "accepted"
}
```

Objects are pulled out of the queue in order of highest priority first. When multiple objects have the same priority, objects are pulled out of the queue in the order they were added. Thus, if we request two items from the queue, we'll get the most recently added object first, followed by the older, lower priority object:

```bash
curl -X GET http://localhost:8000/
{
    "code": 200,
    "data": {
        "contents": "three",
        "elapsed": 283838,
        "priority": 50,
        "sha256": "8b5b9db0c13db24256c829aa364aa90c6d2eba318b9232a4ab9313b954d3555f",
        "uuid": "0b58a347-87e7-4488-92e1-6993968270aa"
    },
    "debug": {
        "in_queue": 1,
        "process_time": 0,
        "proxied": 2,
        "proxy_requests": 2,
        "queue_requests": 3,
        "queue_size": "163 bytes",
        "queued": 3,
        "uptime": 209457
    },
    "status":"ok"
}

curl -X GET http://localhost:8000/
{
    "code": 200,
    "data": {
        "contents": "two",
        "elapsed": 1063087,
        "priority": 10,
        "sha256": "3fc4ccfe745870e2c0d99f71f30ff0656c8dedd41cc1d7d3d376b0dbe685e2f3",
        "uuid": "33c6a5dc-af05-4524-9c0d-0209c592b709"
    },
    "debug": {
        "in_queue": 0,
        "process_time": 0,
        "proxied": 3,
        "proxy_requests": 3,
        "queue_requests": 3,
        "queue_size": "0 bytes",
        "queued": 3,
        "uptime": 248672
    },
    "status":"ok"
}
```

There's nothing else in the queue:

```bash
curl -X GET http://localhost:8000/
{
    "code": 404,
    "reason": "Empty queue.",
    "status": "error"
}
```

Initially based on the Rocket JSON example:
<https://github.com/SergioBenitez/Rocket/tree/v0.4/examples/json>

## TODO

* Implement disk-backing for items in queue over configurable amount of time
