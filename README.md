# RProxy

A notification proxy.

## Notes

Converted the Rocket JSON example to use a simple FIFO queue:
<https://github.com/SergioBenitez/Rocket/tree/v0.4/examples/json>

To test:

Add an element:
```
curl -X POST http://localhost:8000/message -H 'Content-type: application/json' --data '{"contents": "one"}'
{"status":"ok"}
```

Add another element:
```
curl -X POST http://localhost:8000/message -H 'Content-type: application/json' --data '{"contents": "two"}'
{"status":"ok"}
```

Get the first element:
```
curl -X GET http://localhost:8000/message
{"contents":"one"}
```

Get the second element:
```
curl -X GET http://localhost:8000/message
{"contents":"two"}
```

There's nothing else in the queue:
```
curl -X GET http://localhost:8000/message
{"reason":"Resource was not found.","status":"error"}
```