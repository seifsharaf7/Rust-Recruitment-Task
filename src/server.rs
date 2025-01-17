// Importing necessary modules and crates
use crate::message::{EchoMessage, ClientMessage, ServerMessage, AddResponse};
use crate::message::client_message::Message as ClientMessageType;
use crate::message::server_message::Message as ServerMessageType;
use log::{error, info, warn};
use prost::Message;
use std::{
    io::{self, ErrorKind, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

// Define the Client struct to represent a connected client
struct Client {
    stream: TcpStream, // The TCP stream associated with this client
}

impl Client {
    /// Creates a new client instance
    pub fn new(stream: TcpStream) -> Self {
        Client { stream }
    }

    /// Handles communication with the client
    pub fn handle(&mut self) -> io::Result<()> {
        let mut buffer = [0; 512]; // Buffer to store incoming data
        let stream_clone = self.stream.try_clone()?; // Clone the stream for use in a separate thread

        // Spawn a thread for handling client communication
        thread::spawn(move || {
            let mut local_stream = stream_clone;
            loop {
                // Read data from the client
                let bytes_read = match local_stream.read(&mut buffer) {
                    Ok(bytes_read) => bytes_read,
                    Err(e) => {
                        error!("Failed to read from stream: {}", e);
                        break;
                    }
                };

                // If no bytes were read, the client has disconnected
                if bytes_read == 0 {
                    info!("Client disconnected");
                    break;
                }

                // Decode the received message as a ClientMessage
                if let Ok(client_message) = ClientMessage::decode(&buffer[..bytes_read]) {
                    // Check if the message is an AddRequest
                    if let Some(ClientMessageType::AddRequest(add_request)) = client_message.message {
                        let result = add_request.a + add_request.b; // Perform the addition
                        let add_response = AddResponse { result }; // Create the response message

                        let server_message = ServerMessage {
                            message: Some(ServerMessageType::AddResponse(add_response)),
                        };

                        // Encode the response and send it back to the client
                        let payload = server_message.encode_to_vec();
                        if let Err(e) = local_stream.write_all(&payload) {
                            error!("Failed to write AddResponse to stream: {}", e);
                            break;
                        }
                    }
                }

                // Decode the received message as an EchoMessage
                if let Ok(message) = EchoMessage::decode(&buffer[..bytes_read]) {
                    info!("Received: {}", message.content);
                    println!("Received: {}", message.content);

                    // Echo the message back to the client
                    let payload = message.encode_to_vec();
                    if let Err(e) = local_stream.write_all(&payload) {
                        error!("Failed to write to stream: {}", e);
                        break;
                    }
                    if let Err(e) = local_stream.flush() {
                        error!("Failed to flush stream: {}", e);
                        break;
                    }
                } else {
                    error!("Failed to decode message");
                }

                // Clear the buffer to ensure old messages don't interfere
                local_stream.set_nonblocking(true).unwrap();
                while local_stream.read(&mut buffer).is_ok() {}
                local_stream.set_nonblocking(false).unwrap();
            }
        });

        Ok(())
    }
}

// Define the Server struct to represent the server
pub struct Server {
    listener: TcpListener, // TCP listener for incoming connections
    is_running: Arc<AtomicBool>, // Flag to indicate if the server is running
}

impl Server {
    /// Creates a new server instance
    pub fn new(addr: &str) -> io::Result<Self> {
        let listener = TcpListener::bind(addr)?; // Bind to the specified address
        let is_running = Arc::new(AtomicBool::new(false));
        Ok(Server { listener, is_running })
    }

    /// Runs the server, accepting and handling client connections
    pub fn run(&self) -> io::Result<()> {
        self.is_running.store(true, Ordering::SeqCst); // Set the server as running
        info!("Server is running on {}", self.listener.local_addr()?);

        self.listener.set_nonblocking(true)?; // Set listener to non-blocking mode

        while self.is_running.load(Ordering::SeqCst) {
            match self.listener.accept() {
                Ok((stream, addr)) => {
                    info!("New client connected: {}", addr);

                    // Clone the `is_running` flag for the client thread
                    let is_running_clone = Arc::clone(&self.is_running);

                    // Spawn a thread to handle the client
                    thread::spawn(move || {
                        let mut client = Client::new(stream);

                        while is_running_clone.load(Ordering::SeqCst) {
                            if let Err(e) = client.handle() {
                                error!("Error handling client: {}", e);
                                break;
                            }
                        }

                        info!("Client at {} disconnected", addr);
                    });
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                    // No incoming connections, sleep briefly to reduce CPU usage
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    error!("Error accepting connection: {}", e);
                }
            }
        }

        info!("Server stopped.");
        Ok(())
    }

    /// Stops the server
    pub fn stop(&self) {
        if self.is_running.load(Ordering::SeqCst) {
            self.is_running.store(false, Ordering::SeqCst); // Set the running flag to false
            info!("Shutdown signal sent.");
        } else {
            warn!("Server was already stopped or not running.");
        }
    }
}
