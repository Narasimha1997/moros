use crate::{sys, usr};
use crate::api::console::Style;
use crate::api::random;
use crate::api::syscall;
use alloc::borrow::ToOwned;
use alloc::string::String;
use alloc::vec;
use core::str::{self, FromStr};
use core::time::Duration;
use smoltcp::socket::{SocketSet, TcpSocket, TcpSocketBuffer};
use smoltcp::time::Instant;
use smoltcp::wire::IpAddress;

#[derive(Debug)]
struct URL {
    pub host: String,
    pub port: u16,
    pub path: String,
}

impl URL {
    pub fn parse(url: &str) -> Option<Self> {
        if !url.starts_with("http://") {
            return None;
        }
        let url = &url[7..];
        let (server, path) = match url.find('/') {
            Some(i) => url.split_at(i),
            None => (url, "/"),
        };
        let (host, port) = match server.find(':') {
            Some(i) => server.split_at(i),
            None => (server, ":80"),
        };
        let port = &port[1..];
        Some(Self {
            host: host.into(),
            port: port.parse().unwrap_or(80),
            path: path.into(),
        })
    }
}

pub fn main(args: &[&str]) -> usr::shell::ExitCode {
    // Parse command line options
    let mut is_verbose = false;
    let mut host = "";
    let mut path = "";
    let n = args.len();
    for i in 1..n {
        match args[i] {
            "-h" | "--help" => {
                return help();
            }
            "--verbose" => {
                is_verbose = true;
            }
            _ if args[i].starts_with("--") => {
                eprintln!("Invalid option '{}'", args[i]);
                return usr::shell::ExitCode::CommandError;
            }
            _ if host.is_empty() => {
                host = args[i]
            }
            _ if path.is_empty() => {
                path = args[i]
            }
            _ => {
                eprintln!("Too many arguments");
                return usr::shell::ExitCode::CommandError;
            }
        }
    }

    if host.is_empty() && path.is_empty() {
        eprintln!("Missing URL");
        return usr::shell::ExitCode::CommandError;
    } else if path.is_empty() {
        if let Some(i) = args[1].find('/') {
            (host, path) = host.split_at(i);
        } else {
            path = "/"
        }
    }

    let url = "http://".to_owned() + host + path;
    let url = URL::parse(&url).expect("invalid URL format");

    let address = if url.host.ends_with(char::is_numeric) {
        IpAddress::from_str(&url.host).expect("invalid address format")
    } else {
        match usr::host::resolve(&url.host) {
            Ok(ip_addr) => {
                ip_addr
            }
            Err(e) => {
                eprintln!("Could not resolve host: {:?}", e);
                return usr::shell::ExitCode::CommandError;
            }
        }
    };

    let tcp_rx_buffer = TcpSocketBuffer::new(vec![0; 1024]);
    let tcp_tx_buffer = TcpSocketBuffer::new(vec![0; 1024]);
    let tcp_socket = TcpSocket::new(tcp_rx_buffer, tcp_tx_buffer);

    let mut sockets = SocketSet::new(vec![]);
    let tcp_handle = sockets.add(tcp_socket);

    enum State { Connect, Request, Response }
    let mut state = State::Connect;

    if let Some(ref mut iface) = *sys::net::IFACE.lock() {
        match iface.ipv4_addr() {
            None => {
                eprintln!("Error: Interface not ready");
                return usr::shell::ExitCode::CommandError;
            }
            Some(ip_addr) if ip_addr.is_unspecified() => {
                eprintln!("Error: Interface not ready");
                return usr::shell::ExitCode::CommandError;
            }
            _ => {}
        }

        let mut is_header = true;
        let timeout = 5.0;
        let started = syscall::realtime();
        loop {
            if syscall::realtime() - started > timeout {
                eprintln!("Timeout reached");
                return usr::shell::ExitCode::CommandError;
            }
            if sys::console::end_of_text() {
                eprintln!();
                return usr::shell::ExitCode::CommandError;
            }
            let timestamp = Instant::from_millis((syscall::realtime() * 1000.0) as i64);
            match iface.poll(&mut sockets, timestamp) {
                Err(smoltcp::Error::Unrecognized) => {}
                Err(e) => {
                    eprintln!("Network Error: {}", e);
                }
                Ok(_) => {}
            }

            {
                let mut socket = sockets.get::<TcpSocket>(tcp_handle);

                state = match state {
                    State::Connect if !socket.is_active() => {
                        let local_port = 49152 + random::get_u16() % 16384;
                        if is_verbose {
                            println!("* Connecting to {}:{}", address, url.port);
                        }
                        if socket.connect((address, url.port), local_port).is_err() {
                            eprintln!("Could not connect to {}:{}", address, url.port);
                            return usr::shell::ExitCode::CommandError;
                        }
                        State::Request
                    }
                    State::Request if socket.may_send() => {
                        let http_get = "GET ".to_owned() + &url.path + " HTTP/1.1\r\n";
                        let http_host = "Host: ".to_owned() + &url.host + "\r\n";
                        let http_ua = "User-Agent: MOROS/".to_owned() + env!("CARGO_PKG_VERSION") + "\r\n";
                        let http_connection = "Connection: close\r\n".to_owned();
                        if is_verbose {
                            print!("> {}", http_get);
                            print!("> {}", http_host);
                            print!("> {}", http_ua);
                            print!("> {}", http_connection);
                            println!(">");
                        }
                        socket.send_slice(http_get.as_ref()).expect("cannot send");
                        socket.send_slice(http_host.as_ref()).expect("cannot send");
                        socket.send_slice(http_ua.as_ref()).expect("cannot send");
                        socket.send_slice(http_connection.as_ref()).expect("cannot send");
                        socket.send_slice(b"\r\n").expect("cannot send");
                        State::Response
                    }
                    State::Response if socket.can_recv() => {
                        socket.recv(|data| {
                            let contents = String::from_utf8_lossy(data);
                            for line in contents.lines() {
                                if is_header {
                                    if line.is_empty() {
                                        is_header = false;
                                    }
                                    if is_verbose {
                                        println!("< {}", line);
                                    }
                                } else {
                                    println!("{}", line);
                                }
                            }
                            (data.len(), ())
                        }).unwrap();
                        State::Response
                    }
                    State::Response if !socket.may_recv() => {
                        break;
                    }
                    _ => state
                };
            }

            if let Some(wait_duration) = iface.poll_delay(&sockets, timestamp) {
                let wait_duration: Duration = wait_duration.into();
                syscall::sleep(wait_duration.as_secs_f64());
            }
        }
        usr::shell::ExitCode::CommandSuccessful
    } else {
        usr::shell::ExitCode::CommandError
    }
}

fn help() -> usr::shell::ExitCode {
    let csi_option = Style::color("LightCyan");
    let csi_title = Style::color("Yellow");
    let csi_reset = Style::reset();
    println!("{}Usage:{} http {}<options> <url>{1}", csi_title, csi_reset, csi_option);
    println!();
    println!("{}Options:{}", csi_title, csi_reset);
    println!("  {0}--verbose{1}    Increase verbosity", csi_option, csi_reset);
    usr::shell::ExitCode::CommandSuccessful
}
