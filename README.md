# RProxy

A notification proxy.

## Notes

Starting with a simple Rocket example:
<https://github.com/SergioBenitez/Rocket/tree/v0.4/examples/json>

To test:

Add an element:
```
curl -X POST http://localhost:8000/message/1 -H 'Content-type: application/json' --data '{"contents": "one"}'
{"status":"ok"}
```

Can't add the same element twice:
```
curl -X POST http://localhost:8000/message/1 -H 'Content-type: application/json' --data '{"contents": "one"}'
{"reason":"ID exists. Try put.","status":"error"}
```

Add another element:
```
curl -X POST http://localhost:8000/message/2 -H 'Content-type: application/json' --data '{"contents": "one"}'
{"status":"ok"}
```

Get the element:
```
curl -X GET http://localhost:8000/message/2
{"id":2,"contents":"one"}
```

Fix (update) the element:
```
curl -X PUT http://localhost:8000/message/2 -H 'Content-type: application/json' --data '{"contents": "two"}'
{"status":"ok"}
```

Confirm it worked:
```
curl -X GET http://localhost:8000/message/2
{"id":2,"contents":"two"}
```