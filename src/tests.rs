use crate::rocket;
use rocket::local::Client;
use rocket::http::{Status, ContentType};

#[test]
fn invalid_content() {
    let client = Client::new(rocket()).unwrap();

    // Try to get a message when the queue is empty.
    let mut res = client.get("/message").header(ContentType::JSON).dispatch();
    assert_eq!(res.status(), Status::NotFound);
    let body = res.body_string().unwrap();
    assert!(body.contains("error"));
    assert!(body.contains("Resource was not found."));

    // Try to get a message with an invalid URL.
    let mut res = client.get("/message/nothing/here").header(ContentType::JSON).dispatch();
    let body = res.body_string().unwrap();
    assert_eq!(res.status(), Status::NotFound);
    assert!(body.contains("error"));

    // Try to put a message without a proper body.
    let res = client.post("/message").header(ContentType::JSON).dispatch();
    assert_eq!(res.status(), Status::BadRequest);
}

#[test]
fn post_and_get() {
    let client = Client::new(rocket()).unwrap();

    // Start with an empty queue.
    let res = client.get("/message").header(ContentType::JSON).dispatch();
    assert_eq!(res.status(), Status::NotFound);

    // Add an item to the queue
    let res = client.post("/message")
        .header(ContentType::JSON)
        .body(r#"{ "contents": "Item one" }"#)
        .dispatch();
    assert_eq!(res.status(), Status::Ok);

    // Add a second item to the queue
    let res = client.post("/message")
        .header(ContentType::JSON)
        .body(r#"{ "contents": "Item two" }"#)
        .dispatch();
    assert_eq!(res.status(), Status::Ok);

    // Retrieve the first item from the queue
    let mut res = client.get("/message").header(ContentType::JSON).dispatch();
    assert_eq!(res.status(), Status::Ok);
    let body = res.body().unwrap().into_string().unwrap();
    assert!(body.contains("Item one"));

    // Retrieve the second item from the queue
    let mut res = client.get("/message").header(ContentType::JSON).dispatch();
    assert_eq!(res.status(), Status::Ok);
    let body = res.body().unwrap().into_string().unwrap();
    assert!(body.contains("Item two"));

    // The queue is empty again
    let res = client.get("/message").header(ContentType::JSON).dispatch();
    assert_eq!(res.status(), Status::NotFound);

    // We can add another item to the queue
    let res = client.post("/message")
        .header(ContentType::JSON)
        .body(r#"{ "contents": "Item three" }"#)
        .dispatch();
    assert_eq!(res.status(), Status::Ok);

    // Retrieve the third item from the queue
    let mut res = client.get("/message").header(ContentType::JSON).dispatch();
    assert_eq!(res.status(), Status::Ok);
    let body = res.body().unwrap().into_string().unwrap();
    assert!(body.contains("Item three"));

    // The queue is empty again
    let res = client.get("/message").header(ContentType::JSON).dispatch();
    assert_eq!(res.status(), Status::NotFound);
}