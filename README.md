# RQueue

A prioritized notifications queing proxy.

If not otherwise specified, notifications have a priority of 10. Higher priority notifications will be delivered before lower priority notifications.

## Structure

### Message in

* `contents` must be set, and can contain any string. It is generally assumed this will be an encrypted blog.
* `priority` is optional, and can contain an unsigned integer value from 0 to 255; if empty it will automatically be set to 10.

```json
{
    "contents": "String",
    "priority": 10,
}
```

### Message out

The following fields are added by the queue:

* `elapsed` indicates how many milliseconds the message was held in the queue
* `uuid` is an auto-generated unique identifier assigned to each item in the queue

Resulting in the following structure:

```json
{
    "uuid": "String",
    "priority": 10,
    "contents": "String",
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

Now, for example, add an object to the queue:

```bash
curl -X POST http://localhost:8000/ -H 'Content-type: application/json' --data '{"contents": "one"}'
{
    "code": 202,
    "debug": {
        "in_queue": 1,
        "process_time": 2,
        "proxied": 0,
        "proxy_requests": 0,
        "queue_requests": 1,
        "queued": 1,
        "uptime": 9558
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
* `queued` indicates how many times an item has been successfully stored in the queue
* `uptime` indicates how many milliseconds the rqueue daemon has been running

Now, we can add another object:

```bash
curl -X POST http://localhost:8000/ -H 'Content-type: application/json' --data '{"contents": "two"}'
{
    "code": 202,
    "debug": {
        "in_queue": 2,
        "process_time": 2,
        "proxied": 0,
        "proxy_requests": 0,
        "queue_requests": 2,
        "queued": 2,
        "uptime": 267122
    },
    "status": "accepted"
}
```

Now, we can grab the first object from the queue. It has been auto-assigned a priority of 10 becauase we didn't assign a different priority:

```bash
curl -X GET http://localhost:8000/
{"contents":"one","priority":10,"elapsed":31159}
{
    "code": 200,
    "data": {
        "contents": "one",
        "elapsed": 336598,
        "priority": 10,
        "uuid": "ac782fc0-1fae-42e5-83a3-790e9c63a122"
    },
    "debug": {
        "in_queue": 1,
        "process_time": 0,
        "proxied": 1,
        "proxy_requests": 1,
        "queue_requests": 2,
        "queued": 2,
        "uptime": 346154
    },
    "status":"ok"
}
```

The debug array contains the same information displayed for POSTs. The data array includes some new information:

* `priority` was auto-assigned to 10, as no value was manually assigned when the data was POSTed
* `uuid` was auto-assigned to a random version 4 UUID, uniquely identifying this specific contents

A higher priority object can be added to the queue by including the `priority` parameter:

```bash
curl -X POST http://localhost:8000/ -H 'Content-type: application/json' --data '{"contents": "three", "priority": 50}'
{
    "code": 202,
    "debug": {
        "in_queue": 2,
        "process_time": 2,
        "proxied": 1,
        "proxy_requests": 1,
        "queue_requests": 3,
        "queued": 3,
        "uptime": 1044214
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
        "uuid": "0b58a347-87e7-4488-92e1-6993968270aa"
    },
    "debug": {
        "in_queue": 1,
        "process_time": 0,
        "proxied": 2,
        "proxy_requests": 2,
        "queue_requests": 3,
        "queued": 3,
        "uptime": 1328051
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
        "uuid": "33c6a5dc-af05-4524-9c0d-0209c592b709"
    },
    "debug": {
        "in_queue": 0,
        "process_time": 0,
        "proxied": 3,
        "proxy_requests": 3,
        "queue_requests": 3,
        "queued": 3,
        "uptime": 1330207
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

## TODO

* Provide better error handling for invalid priority
* Optionally require configurable JWT authentication to post to queue
* Implement method for pushing queued data via HTTP/S
* Implement disk-backing for items in queue over configurable amount of time
* Add configuration for enabling/disabling debug output
* Add configurable logging
* Add configuration to cap memory usage (and track memory usage)
* Add configuration to cap storage usage (and track storage useage) for disk-backing
