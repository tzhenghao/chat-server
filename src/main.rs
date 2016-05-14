extern crate mio;

extern crate http_muncher;
use http_muncher::{Parser, ParserHandler};

use std::net::SocketAddr;

use mio::*;
use mio::tcp::*;

use std::cell::RefCell;
use std::rc::Rc;

extern crate sha1;
extern crate rustc_serialize;

use rustc_serialize::base64::{ToBase64, STANDARD};

use std::collections::HashMap;

use std::fmt;

#[derive(PartialEq)]
enum ClientState {
    AwaitingHandshake,
    HandshakeResponse,
    Connected
}

fn gen_key(key: &String) -> String {
    let mut m = sha1::Sha1::new();

    let mut buf = [0u8; 20];

    m.update(key.as_bytes());
    m.update("258EAFA5-E914-47DA-95CA-C5AB0DC85B11".as_bytes());

    m.output(&mut buf);

    return buf.to_base64(STANDARD);
}

struct WebSocketServer {
    socket: TcpListener,
    clients: HashMap<Token, TcpStream>,
    token_Counter: usize
}

struct HttpParser {
    current_key: Option<String>,
    headers: Rc<RefCell<HashMap<String, String>>>
}

impl ParserHandler for HttpParser {
    fn on_header_field(&mut self, s: &[u8]) -> bool {
        self.current_key = Some(std::str::from_utf(s).unwrap().to_string());
        true
    }


    fn on_header_value(&mut self, s: &[u8]) -> bool {
        self.headers.borrow_mut()
            .insert(self.current_key.clone().unwrap(),
                    std::str::from_utf8(s).unwrap().to_string());
        true
    }

    fn on_headers_complete(&mut self) -> bool {
        false
    }
}

struct WebSocketClient {
    socket: TcpStream,
    http_parser: Parser<HttpParser>,
    // Adding headers declaration to the WebSocketClient struct.
    headers: Rc<RefCell<HashMap<String, String>>>,
    // Adding a new 'interest' property:
    interest: EventSet,
    // Adding a client state:
    state: ClientState
}

impl WebSocketClient {

    fn new(socket: TcpStream) -> WebSocketClient {
        let headers = Rc::new(RefCell::new(HashMap::new()));

        WebSocketClient {
            socket: socket,

            // Making first clone of the 'headers' variable to read its contents.
            headers: headers.clone(),

            http_parser: Parser::request(HttpParser {
                current_key: None,

                // ...and the second clone to write new headers to it:
                headers: headers.clone()
            })

            // Initial events that interest us
            interest: EventSet::readable(),

            // Initial state
            state: ClientState::AwaitingHandshake
        }
    }

    fn read(&mut self) {
        loop {
            let mut buf = [0; 2048];
            match self.socket.try_read(&mut buf) {
                Err(e) => {
                    println!("Error while reading socket: {:?}", e);
                    return
                },
                Ok(None) =>
                    // Socket buffer has got no more bytes.
                    break,
                Ok(Some(len)) => {
                    self.http_parser.parse(&buf[0..len]);
                    if self.http_parser.is_upgrade() {

                        // CHange the current state
                        self.state = ClientState::HandshakeResponse;

                        // Change current interest to 'Writable'
                        self.interest.remove(EventSet::readable());
                        self.interest.insert(EventSet::writable());

                        break;
                    }
                }
            }
        }
    }

	fn write(&mut self) {

		// get headers HashMap from Rc wrapper
		let headers = self.headers.borrow();

		let response_key = gen_key(&headers.get("Sec-WebSocket-Key").unwrap());

		let response = fmt::format(format_args!("HTTP/1.1 101 Switching Protocols\r\n\
												Connection: Upgrade\r\n\
												Sec-WebSocket-Accept: {}\r\n\
												Upgrade: websocket\r\n\r\n", response_key))

		// write response to the socket::
		self.socket.try_write(response.as_bytes()).unwrap();

		// change state
		self.state = ClientState::Conected;

		// change the interest back to 'readable()':
		self.interest.remove(EventSet::writable());
		self.interest.insert(EventSet::readable());
	}
}

const SERVER_TOKEN: Token = Token(0);

impl Handler for WebSocketServer {

    type Timeout = usize;
    type Message = ();

    fn ready(&mut self, event_loop: &mut EventLoop<WebSocketServer>,
            token: Token, events: EventSet)
    {
        // Are we dealing with read event?
        if events.is_readable() {
            match token {
                SERVER_TOKEN => {
                    let client_socket = match self.socket.accept() {
                        Err(e) => {
                            println!("Accept error: {}", e);
                            return;
                        },
                        Ok(None) => unreachable!("Accept has returned 'None'"),
                        Ok(Some((sock, addr))) => sock
                    };

                    self.token_counter += 1;
                    let new_token = Token(self.token_counter);
                    self.clients.insert(new_token, WebSocketClient::new(client_socket));

                    event_loop.register(&self.clients[&new_token].socket,
                                        new_token, EventSet::readable(),
                                        PollOpt::edge() | PollOpt::oneshot()).unwrap();
                },
                token => {
                    let mut client = self.clients.get_mut(&token).unwrap();
                    client.read();
                    event_loop.reregister(&client.socket, token, client.interest,
                            PollOpt::edge() | PollOpt::oneshot()).unwrap();
                }
            }
        }

        // Handle write events that are generated whenever the socket becomes
        // available for a write operation.
        if events.is_writable() {
            let mut client = self.clients.get_mut(&token).unwrap();
            client.write();
            event_loop.reregister(&client.socket, token, client.interest,
                                    PollOpt::edge() | PollOpt::oneshot()).unwrap();
        }
    }
}

fn main() {

    let mut event_loop = EventLoop::new().unwrap();

    let mut server = WebSocketServer {
        token_counter: 1,
        clients: HashMap::new(),
        socket: server_socket
    };

    println!("Hello, world!");

    let address = "0.0.0.0:10000".parse::<SocketAddr>().unwrap();
    let server_socket = TcpListener::bind(&address).unwrap();

    event_loop.register(&server.socket, Token(0), EventSet::readable(),
                        PollOpt::edge()).unwrap();

    event_loop.run(&mut server).unwrap();
}
