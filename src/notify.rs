use std::thread;
use std::time::Duration;

use lettre_email::{Email};
use lettre::smtp::authentication::{Credentials, Mechanism};
use lettre::{Transport, SmtpClient};
use lettre::smtp::extension::ClientId;
use lettre::smtp::ConnectionReuseParameters;

use crate::{NOTIFY_CONFIG, DEFAULT_DELAY, QUEUE, milliseconds_since_timestamp, MessageInternal};

pub fn notify_loop(server_started: Duration) {
    let mut sleep_time = DEFAULT_DELAY;
    loop {
        log::debug!("{}|top of notify loop", milliseconds_since_timestamp(server_started));
        thread::sleep(Duration::from_secs(sleep_time as u64));

        let queue_contents;
        // We preserve a copy of the message in case there's an error, as then we'll
        // return it to the queue.
        let mut message: MessageInternal = MessageInternal::default();
        {
            // We don't use counters here, but we have to grab locks in order to prevent a race
            let mut queue = QUEUE.lock().expect("queue lock");
            queue_contents = queue.pop().map(|internal| {
                message.size_in_bytes = internal.0.size_in_bytes;
                message.contents = internal.0.contents.clone();
                message.sha256 = internal.0.sha256.clone();
                message.priority = internal.0.priority;
                message.arrived = internal.0.arrived;
                message.uuid = internal.0.uuid.clone();
                message.original_priority = internal.0.original_priority;
                message.delivery_attempts = internal.0.delivery_attempts + 1;
            });
        }

        // Send notifications
        if queue_contents != None {
            sleep_time = 0;
            log::debug!("{}|message from queue with sha256 {}: '{}'",
                milliseconds_since_timestamp(server_started),
                &message.sha256,
                &message.contents,
            );

            let notify_config = NOTIFY_CONFIG.lock().unwrap();
            let email = Email::builder()
                .from((&notify_config.mail_from_address.to_string(), &notify_config.mail_from_name.to_string()))
                .to((&notify_config.mail_to_address.to_string(), &notify_config.mail_to_name.to_string()))
                .subject("test notification")
                .alternative("<body><p>This is a <em>test</em> notification.</p></body>", "This is a test notification.")
                .build()
                .expect("failed to create email");
            
            let smtp_user = &notify_config.smtp_user;
            let smtp_password = &notify_config.smtp_password;
            let mut mailer = SmtpClient::new_simple(&notify_config.smtp_server.to_string()).unwrap()
                // Set the name sent during EHLO/HELO, default is `localhost`
                .hello_name(ClientId::Domain("localhost".to_string()))
                // Add credentials for authentication
                .credentials(Credentials::new(smtp_user.to_string(), smtp_password.to_string()))
                // Enable SMTPUTF8 if the server supports it
                .smtp_utf8(true)
                // Configure expected authentication mechanism
                .authentication_mechanism(Mechanism::Plain)
                // Enable connection reuse
                .connection_reuse(ConnectionReuseParameters::ReuseUnlimited).transport();
            
                let result = mailer.send(email.into());
            
            eprintln!("result {:?}", result);
        }
        else {
            // If the queue is empty, sleep longer.
            let notify_config = NOTIFY_CONFIG.lock().unwrap();
            sleep_time = notify_config.delay;
        }
    }
}