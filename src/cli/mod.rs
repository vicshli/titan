mod parse;

use crate::drop_policy::DropPolicy;
use crate::node::Node;
use crate::protocol::tcp::prelude::{Port, Remote, SocketDescriptor};
use crate::protocol::tcp::{TcpAcceptError, TcpConnError, TcpListenError, TcpSendError};
use crate::protocol::Protocol;
use crate::repl::{HandleUserInput, HandleUserInputError, Repl};
use async_trait::async_trait;
use std::fs::File;
use std::io::Write;
use std::net::Ipv4Addr;
use std::sync::Arc;

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    ListInterface(Option<String>),
    ListRoute(Option<String>),
    ListSockets(Option<String>),
    InterfaceDown(u16),
    InterfaceUp(u16),
    SendIPv4Packet {
        virtual_ip: Ipv4Addr,
        protocol: Protocol,
        payload: String,
    },
    SendTCPPacket(SocketDescriptor, Vec<u8>),
    OpenListenSocket(Port),
    ConnectSocket(Ipv4Addr, Port),
    ReadSocket {
        descriptor: SocketDescriptor,
        num_bytes: usize,
        would_block: bool,
    },
    Shutdown(SocketDescriptor, TcpShutdownKind),
    Close(SocketDescriptor),
    SendFile {
        path: String,
        dest_ip: Ipv4Addr,
        port: Port,
    },
    RecvFile {
        out_path: String,
        port: Port,
    },
    Quit,
    None,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TcpShutdownKind {
    Read,
    Write,
    ReadWrite,
}

#[derive(Debug)]
pub enum SendFileError {
    OpenFile(std::io::Error),
    ReadFile(std::io::Error),
    Connect(TcpConnError),
    Send(TcpSendError),
}

#[derive(Debug)]
pub enum RecvFileError {
    FileIo(std::io::Error),
    Listen(TcpListenError),
    Accept(TcpAcceptError),
}

impl From<std::io::Error> for RecvFileError {
    fn from(e: std::io::Error) -> Self {
        RecvFileError::FileIo(e)
    }
}

pub struct Cli<DP: DropPolicy> {
    node: Arc<Node<DP>>,
}

#[async_trait]
impl<DP: DropPolicy> HandleUserInput for Cli<DP> {
    async fn handle(
        &mut self,
        user_input: String,
    ) -> Result<(), crate::repl::HandleUserInputError> {
        match parse::parse_command(user_input) {
            Ok(Command::Quit) => return Err(HandleUserInputError::Terminate),
            Ok(Command::None) => (),
            Ok(cmd) => self.execute_command(cmd).await,
            Err(e) => {
                eprintln!("{e}");
            }
        };

        Ok(())
    }
}

impl<DP: DropPolicy> Cli<DP> {
    pub fn new(node: Arc<Node<DP>>) -> Self {
        Self { node }
    }

    pub async fn run(self) {
        let h = tokio::spawn(async move {
            Repl::new(self, Some(">> ".into())).serve().await;
        });
        h.await.expect("CLI should not panic");
    }

    async fn execute_command(&self, cmd: Command) {
        match cmd {
            Command::None => (),
            Command::ListInterface(op) => {
                self.print_interfaces(op).await;
            }
            Command::ListRoute(op) => {
                self.print_routes(op).await;
            }
            Command::ListSockets(op) => {
                self.print_sockets(op).await;
            }
            Command::InterfaceDown(interface) => {
                eprintln!("Turning down interface {interface}");
                if let Err(e) = self.node.deactivate(interface).await {
                    eprintln!("Failed to turn interface {interface} down: {e:?}");
                }
            }
            Command::InterfaceUp(interface) => {
                eprintln!("Turning up interface {interface}");
                if let Err(e) = self.node.activate(interface).await {
                    eprintln!("Failed to turn interface {interface} up: {e:?}");
                }
            }
            Command::SendIPv4Packet {
                virtual_ip,
                protocol,
                payload,
            } => {
                eprintln!(
                    "Sending packet \"{payload}\" with protocol {protocol:?} to {virtual_ip}",
                );
                if let Err(e) = self
                    .node
                    .send(payload.as_bytes(), protocol, virtual_ip)
                    .await
                {
                    eprintln!("Failed to send packet: {e:?}");
                }
            }
            Command::SendTCPPacket(socket_descriptor, payload) => {
                self.tcp_send(socket_descriptor, payload).await;
            }
            Command::OpenListenSocket(port) => {
                self.open_listen_socket_on(port).await;
            }
            Command::ConnectSocket(ip, port) => {
                self.connect(ip, port).await;
            }
            Command::ReadSocket {
                descriptor,
                num_bytes,
                would_block,
            } => {
                if would_block {
                    self.tcp_read(descriptor, num_bytes).await;
                } else {
                    self.tcp_bg_read(descriptor, num_bytes).await;
                }
            }

            Command::Shutdown(socket, opt) => {
                self.shutdown(socket, opt).await;
            }
            Command::Close(socket_descriptor) => {
                self.close_socket(socket_descriptor).await;
            }
            Command::SendFile {
                path,
                dest_ip,
                port,
            } => {
                self.send_file(&path, (dest_ip, port));
            }
            Command::RecvFile { out_path, port } => self.recv_file(&out_path, port),
            Command::Quit => {
                eprintln!("Quitting");
            }
        }
    }

    async fn print_interfaces(&self, file: Option<String>) {
        let mut id = 0;
        match file {
            Some(file) => {
                let mut f = File::create(file).unwrap();
                f.write_all(b"id\tstate\tlocal\t\tremote\tport\n").unwrap();
                for link in &*self.node.iter_links().await {
                    f.write_all(format!("{id}\t{link}\n").as_bytes()).unwrap();
                    id += 1;
                }
            }
            None => {
                println!("id\tstate\tlocal\t\tremote\t        port");
                for link in &*self.node.iter_links().await {
                    println!("{id}\t{link}");
                    id += 1;
                }
            }
        }
    }

    async fn print_routes(&self, _file: Option<String>) {
        todo!()
    }

    async fn print_sockets(&self, file: Option<String>) {
        self.node.print_sockets(file).await;
    }

    async fn tcp_send(&self, socket_descriptor: SocketDescriptor, payload: Vec<u8>) {
        if let Err(e) = self.node.tcp_send(socket_descriptor, &payload).await {
            eprintln!(
                "Failed to send on socket {}. Error: {:?}",
                socket_descriptor.0, e
            )
        }
    }

    async fn tcp_bg_read(&self, descriptor: SocketDescriptor, num_bytes: usize) {
        let node = self.node.clone();
        tokio::spawn(async move { tcp_read(&node, descriptor, num_bytes).await });
    }

    async fn tcp_read(&self, descriptor: SocketDescriptor, num_bytes: usize) {
        tcp_read(&self.node, descriptor, num_bytes).await
    }

    async fn shutdown(&self, descriptor: SocketDescriptor, option: TcpShutdownKind) {
        match self.node.get_socket_by_descriptor(descriptor).await {
            Some(socket) => match option {
                TcpShutdownKind::Read => socket.close_read().await,
                TcpShutdownKind::Write => socket.close().await,
                TcpShutdownKind::ReadWrite => socket.close_rw().await,
            },
            None => {
                eprintln!("Socket {} not found", descriptor.0)
            }
        }
    }

    async fn open_listen_socket_on(&self, port: Port) {
        match self.node.listen(port).await {
            Ok(_) => eprintln!("Listen socket opened on port {}", port.0),
            Err(e) => {
                eprintln!("Failed to listen on port {}. Error: {:?}", port.0, e)
            }
        }
    }

    async fn connect(&self, ip: Ipv4Addr, port: Port) {
        match self.node.connect(ip, port).await {
            Ok(conn) => {
                let socket_descriptor = self
                    .node
                    .get_socket_descriptor(conn.socket_id())
                    .await
                    .unwrap();
                eprintln!("Connection established. ID: {}", socket_descriptor.0);
            }
            Err(e) => {
                eprintln!("Failed to connect to {}:{}. Error: {:?}", ip, port.0, e)
            }
        }
    }

    fn send_file(&self, path: &str, remote: impl Into<Remote>) {
        let remote = remote.into();
        let node = self.node.clone();
        let path: String = path.into();
        tokio::spawn(async move {
            match node.send_file(&path, remote).await {
                Ok(_) => {
                    eprintln!("Send file complete.");
                }
                Err(e) => {
                    eprintln!("Failed to send file. Error: {e:?}")
                }
            }
        });
    }

    fn recv_file(&self, out_path: &str, port: Port) {
        let node = self.node.clone();
        let out_path: String = out_path.into();
        tokio::spawn(async move {
            match node.recv_file(&out_path, port).await {
                Ok(_) => {
                    eprintln!("Receive file complete");
                }
                Err(e) => {
                    eprintln!("Failed to receive file. Error: {e:?}")
                }
            }
        });
    }

    async fn close_socket(&self, socket_descriptor: SocketDescriptor) {
        if self
            .node
            .close_socket_by_descriptor(socket_descriptor)
            .await
            .is_err()
        {
            eprintln!(
                "Failed to close socket: socket {} does not exist",
                socket_descriptor.0
            );
        }
    }
}

async fn tcp_read<DP: DropPolicy>(node: &Node<DP>, sid: SocketDescriptor, num_bytes: usize) {
    match node.tcp_read(sid, num_bytes).await {
        Ok(bytes) => {
            println!("{}", String::from_utf8_lossy(&bytes));
        }
        Err(e) => {
            eprintln!("Failed to read: {e:?}");
        }
    }
}
