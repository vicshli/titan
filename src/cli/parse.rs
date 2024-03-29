use std::{fmt::Display, str::SplitWhitespace};

use crate::protocol::{
    tcp::prelude::{Port, SocketDescriptor},
    Protocol,
};

use super::{Command, TcpShutdownKind};

#[derive(Debug, PartialEq, Eq)]
pub enum ParseOpenListenSocketError {
    NoPort,
    InvalidPort,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseDownError {
    InvalidLinkId,
    NoLinkId,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseUpError {
    InvalidLinkId,
    NoLinkId,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseSendError {
    NoIp,
    InvalidIp,
    NoProtocol,
    InvalidProtocol,
    NoPayload,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseConnectError {
    NoIp,
    InvalidIp,
    NoPort,
    InvalidPort,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseTcpSendError {
    NoSocketDescriptor,
    InvalidSocketDescriptor,
    NoPayload,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseTcpReadError {
    NoSocketDescriptor,
    InvalidSocketDescriptor,
    NoNumBytesToRead,
    InvalidNumBytesToRead,
    InvalidBlockingIndicator,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseTcpShutdownError {
    NoSocketDescriptor,
    InvalidSocketDescriptor,
    InvalidShutdownType(String),
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseCloseError {
    NoSocketDescriptor,
    InvalidSocketDescriptor,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseSendFileError {
    NoFile,
    NoIp,
    InvalidIp,
    NoPort,
    InvalidPort,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseRecvFileError {
    NoFile,
    NoPort,
    InvalidPort,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    Unknown,
    Down(ParseDownError),
    Up(ParseUpError),
    Send(ParseSendError),
    OpenListenSocket(ParseOpenListenSocketError),
    Connect(ParseConnectError),
    TcpSend(ParseTcpSendError),
    TcpRead(ParseTcpReadError),
    TcpShutdown(ParseTcpShutdownError),
    TcpClose(ParseCloseError),
    SendFile(ParseSendFileError),
    RecvFile(ParseRecvFileError),
}

impl Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Unknown => write!(f, "Unknown command"),
            ParseError::Down(e) => write!(
                f,
                "Invalid down command. Usage: down <integer>. Error: {e:?}"
            ),
            ParseError::Up(e) => {
                write!(f, "Invalid up command. Usage: up <integer>. Error: {e:?}")
            }
            ParseError::Send(e) => {
                write!(
                    f,
                    "Invalid send command. Usage: send <vip> <proto> <string>. Error: {e:?}"
                )
            }
            ParseError::OpenListenSocket(e) => {
                write!(
                    f,
                    "Invalid open socket command. Usage: a <port>. Error: {e:?}"
                )
            }
            ParseError::Connect(e) => {
                write!(
                    f,
                    "Invalid connect command. Usage: c <ip> <port>. Error: {e:?}"
                )
            }
            ParseError::TcpSend(e) => {
                write!(
                    f,
                    "Invalid send command. Usage: s <socket_id> <data>. Error: {e:?}"
                )
            }
            ParseError::TcpRead(e) => {
                write!(
                    f,
                    "Invalid read command. Usage: r <socket ID> <numbytes> <y|N>. Error: {e:?}"
                )
            }
            ParseError::TcpShutdown(e) => {
                write!(
                    f,
                    "Invalid shutdown command. Usage: sd <socket ID> <read|write|both>. Error: {e:?}"
                )
            }
            ParseError::TcpClose(e) => {
                write!(
                    f,
                    "Invalid close command. Usage: cl <socket ID>. Error: {e:?}"
                )
            }
            ParseError::SendFile(e) => {
                write!(
                    f,
                    "Invalid send file command. Usage: sf <filename> <ip> <port>. Error: {e:?}"
                )
            }
            ParseError::RecvFile(e) => {
                write!(
                    f,
                    "Invalid receive file command. Usage: rf <filename> <port>. Error: {e:?}"
                )
            }
        }
    }
}

impl From<ParseUpError> for ParseError {
    fn from(v: ParseUpError) -> Self {
        ParseError::Up(v)
    }
}

impl From<ParseDownError> for ParseError {
    fn from(v: ParseDownError) -> Self {
        ParseError::Down(v)
    }
}

impl From<ParseSendError> for ParseError {
    fn from(v: ParseSendError) -> Self {
        ParseError::Send(v)
    }
}

impl From<ParseOpenListenSocketError> for ParseError {
    fn from(v: ParseOpenListenSocketError) -> Self {
        ParseError::OpenListenSocket(v)
    }
}

impl From<ParseConnectError> for ParseError {
    fn from(v: ParseConnectError) -> Self {
        ParseError::Connect(v)
    }
}

impl From<ParseTcpSendError> for ParseError {
    fn from(v: ParseTcpSendError) -> Self {
        ParseError::TcpSend(v)
    }
}

impl From<ParseTcpReadError> for ParseError {
    fn from(v: ParseTcpReadError) -> Self {
        ParseError::TcpRead(v)
    }
}

impl From<ParseTcpShutdownError> for ParseError {
    fn from(v: ParseTcpShutdownError) -> Self {
        ParseError::TcpShutdown(v)
    }
}

impl From<ParseCloseError> for ParseError {
    fn from(v: ParseCloseError) -> Self {
        ParseError::TcpClose(v)
    }
}

impl From<ParseSendFileError> for ParseError {
    fn from(v: ParseSendFileError) -> Self {
        ParseError::SendFile(v)
    }
}

impl From<ParseRecvFileError> for ParseError {
    fn from(v: ParseRecvFileError) -> Self {
        ParseError::RecvFile(v)
    }
}

pub fn parse_command(line: String) -> Result<Command, ParseError> {
    let mut tokens = line.split_whitespace();
    let c = tokens.next();
    match c {
        Some(cmd) => parse_cmd(cmd, tokens),
        None => Ok(Command::None),
    }
}

fn parse_cmd(cmd: &str, mut tokens: SplitWhitespace) -> Result<Command, ParseError> {
    match cmd {
        "li" => {
            let arg = tokens.next();
            match arg {
                Some(arg) => Ok(Command::ListInterface(Some(arg.to_string()))),
                None => Ok(Command::ListInterface(None)),
            }
        }
        "interfaces" => {
            let arg = tokens.next();
            match arg {
                Some(arg) => Ok(Command::ListInterface(Some(arg.to_string()))),
                None => Ok(Command::ListInterface(None)),
            }
        }
        "lr" => {
            let arg = tokens.next();
            match arg {
                Some(arg) => Ok(Command::ListRoute(Some(arg.to_string()))),
                None => Ok(Command::ListRoute(None)),
            }
        }
        "routes" => {
            let arg = tokens.next();
            match arg {
                Some(arg) => Ok(Command::ListRoute(Some(arg.to_string()))),
                None => Ok(Command::ListRoute(None)),
            }
        }
        "down" => {
            let arg = tokens.next();
            match arg {
                Some(arg) => {
                    let link_no = arg.parse::<u16>();
                    match link_no {
                        Ok(link_no) => Ok(Command::InterfaceDown(link_no)),
                        Err(_) => Err(ParseDownError::InvalidLinkId.into()),
                    }
                }
                None => Err(ParseDownError::NoLinkId.into()),
            }
        }
        "up" => {
            let arg = tokens.next();
            match arg {
                Some(arg) => {
                    let link_no = arg.parse::<u16>();
                    match link_no {
                        Ok(link_no) => Ok(Command::InterfaceUp(link_no)),
                        Err(_) => Err(ParseUpError::InvalidLinkId.into()),
                    }
                }
                None => Err(ParseUpError::NoLinkId.into()),
            }
        }
        "send" => {
            let virtual_ip = tokens.next().ok_or(ParseSendError::NoIp)?;
            let protocol = tokens.next().ok_or(ParseSendError::NoProtocol)?;

            let mut payload = String::new();
            for token in tokens {
                payload.push_str(token);
            }
            if payload.is_empty() {
                return Err(ParseSendError::NoPayload.into());
            }

            let virtual_ip = virtual_ip.parse().map_err(|_| ParseSendError::InvalidIp)?;
            let protocol =
                Protocol::try_from(protocol).map_err(|_| ParseSendError::InvalidProtocol)?;
            Ok(Command::SendIPv4Packet {
                virtual_ip,
                protocol,
                payload,
            })
        }
        "ls" => {
            let arg = tokens.next();
            match arg {
                Some(arg) => Ok(Command::ListSockets(Some(arg.to_string()))),
                None => Ok(Command::ListSockets(None)),
            }
        }
        "a" => {
            let arg = tokens.next().ok_or(ParseOpenListenSocketError::NoPort)?;
            let port = arg
                .parse::<u16>()
                .map_err(|_| ParseOpenListenSocketError::InvalidPort)?;
            Ok(Command::OpenListenSocket(Port(port)))
        }
        "c" => {
            let ip = tokens.next().ok_or(ParseConnectError::NoIp)?;
            let ip = ip.parse().map_err(|_| ParseConnectError::InvalidIp)?;
            let port = tokens.next().ok_or(ParseConnectError::NoPort)?;
            let port: u16 = port.parse().map_err(|_| ParseConnectError::InvalidPort)?;
            Ok(Command::ConnectSocket(ip, port.into()))
        }
        "s" => {
            let sid = tokens.next().ok_or(ParseTcpSendError::NoSocketDescriptor)?;
            let sid = SocketDescriptor(
                sid.parse()
                    .map_err(|_| ParseTcpSendError::InvalidSocketDescriptor)?,
            );
            let payload = tokens.next().ok_or(ParseTcpSendError::NoPayload)?;
            Ok(Command::SendTCPPacket(sid, payload.as_bytes().into()))
        }
        "r" => {
            let sid = tokens.next().ok_or(ParseTcpReadError::NoSocketDescriptor)?;
            let sid = SocketDescriptor(
                sid.parse()
                    .map_err(|_| ParseTcpReadError::InvalidSocketDescriptor)?,
            );
            let num_bytes: usize = tokens
                .next()
                .ok_or(ParseTcpReadError::NoNumBytesToRead)?
                .parse()
                .map_err(|_| ParseTcpReadError::InvalidNumBytesToRead)?;

            let maybe_blocking = tokens.next();
            let would_block = match maybe_blocking {
                Some(token) => match token {
                    "y" => true,
                    "N" => false,
                    _ => return Err(ParseTcpReadError::InvalidBlockingIndicator.into()),
                },
                None => false,
            };

            Ok(Command::ReadSocket {
                descriptor: sid,
                num_bytes,
                would_block,
            })
        }
        "sd" => {
            let sid = tokens
                .next()
                .ok_or(ParseTcpShutdownError::NoSocketDescriptor)?;
            let sid = SocketDescriptor(
                sid.parse()
                    .map_err(|_| ParseTcpShutdownError::InvalidSocketDescriptor)?,
            );

            let maybe_option = tokens.next();
            let opt = match maybe_option {
                Some(token) => match token {
                    "w" | "write" => TcpShutdownKind::Write,
                    "r" | "read" => TcpShutdownKind::Read,

                    "both" => TcpShutdownKind::ReadWrite,
                    _ => {
                        return Err(ParseTcpShutdownError::InvalidShutdownType(token.into()).into())
                    }
                },
                None => TcpShutdownKind::Write,
            };

            Ok(Command::Shutdown(sid, opt))
        }
        "cl" => {
            let sid = tokens.next().ok_or(ParseCloseError::NoSocketDescriptor)?;
            let sid = SocketDescriptor(
                sid.parse()
                    .map_err(|_| ParseCloseError::InvalidSocketDescriptor)?,
            );

            Ok(Command::Close(sid))
        }
        "sf" => {
            let filename = tokens.next().ok_or(ParseSendFileError::NoFile)?;
            let ip = tokens
                .next()
                .ok_or(ParseSendFileError::NoIp)?
                .parse()
                .map_err(|_| ParseSendFileError::InvalidIp)?;
            let port = tokens
                .next()
                .ok_or(ParseSendFileError::NoPort)?
                .parse::<u16>()
                .map_err(|_| ParseSendFileError::InvalidPort)?
                .into();

            Ok(Command::SendFile {
                path: filename.into(),
                dest_ip: ip,
                port,
            })
        }
        "rf" => {
            let filename = tokens.next().ok_or(ParseRecvFileError::NoFile)?;
            let port = tokens
                .next()
                .ok_or(ParseRecvFileError::NoPort)?
                .parse::<u16>()
                .map_err(|_| ParseRecvFileError::InvalidPort)?
                .into();
            Ok(Command::RecvFile {
                out_path: filename.into(),
                port,
            })
        }
        "q" => Ok(Command::Quit),
        _ => Err(ParseError::Unknown),
    }
}

#[cfg(test)]
mod tests {

    use std::net::Ipv4Addr;

    use super::*;

    #[test]
    fn parse_connect() {
        assert_eq!(
            parse_command("c".into()).unwrap_err(),
            ParseConnectError::NoIp.into()
        );

        assert_eq!(
            parse_command("c 1".into()).unwrap_err(),
            ParseConnectError::InvalidIp.into()
        );

        assert_eq!(
            parse_command("c 1.2.3.4".into()).unwrap_err(),
            ParseConnectError::NoPort.into()
        );

        assert_eq!(
            parse_command("c 1.2.3.4 ss".into()).unwrap_err(),
            ParseConnectError::InvalidPort.into()
        );

        let c = parse_command("c 1.2.3.4 33".into()).unwrap();
        let expected = Command::ConnectSocket(Ipv4Addr::new(1, 2, 3, 4), 33u16.into());
        assert_eq!(c, expected);
    }

    #[test]
    fn parse_tcp_send() {
        assert_eq!(
            parse_command("s".into()).unwrap_err(),
            ParseTcpSendError::NoSocketDescriptor.into(),
        );

        assert_eq!(
            parse_command("s ssss".into()).unwrap_err(),
            ParseTcpSendError::InvalidSocketDescriptor.into(),
        );

        assert_eq!(
            parse_command("s 33".into()).unwrap_err(),
            ParseTcpSendError::NoPayload.into(),
        );

        let c = parse_command("s 33 heehee".into()).unwrap();
        let expected = Command::SendTCPPacket(
            SocketDescriptor(33),
            String::from("heehee").as_bytes().into(),
        );
        assert_eq!(c, expected);
    }

    #[test]
    fn parse_tcp_read() {
        assert_eq!(
            parse_command("r".into()).unwrap_err(),
            ParseTcpReadError::NoSocketDescriptor.into(),
        );

        assert_eq!(
            parse_command("r ssss".into()).unwrap_err(),
            ParseTcpReadError::InvalidSocketDescriptor.into(),
        );

        assert_eq!(
            parse_command("r 33".into()).unwrap_err(),
            ParseTcpReadError::NoNumBytesToRead.into(),
        );

        assert_eq!(
            parse_command("r 33 heehee".into()).unwrap_err(),
            ParseTcpReadError::InvalidNumBytesToRead.into()
        );

        assert_eq!(
            parse_command("r 33 100 n".into()).unwrap_err(),
            ParseTcpReadError::InvalidBlockingIndicator.into()
        );

        let c = parse_command("r 33 100 y".into()).unwrap();
        assert_eq!(
            c,
            Command::ReadSocket {
                descriptor: SocketDescriptor(33),
                num_bytes: 100,
                would_block: true
            }
        );
        let c = parse_command("r 33 100 N".into()).unwrap();
        assert_eq!(
            c,
            Command::ReadSocket {
                descriptor: SocketDescriptor(33),
                num_bytes: 100,
                would_block: false
            }
        );
        let c = parse_command("r 33 100".into()).unwrap();
        assert_eq!(
            c,
            Command::ReadSocket {
                descriptor: SocketDescriptor(33),
                num_bytes: 100,
                would_block: false
            }
        );
    }

    #[test]
    fn parse_tcp_shutdown() {
        assert_eq!(
            parse_command("sd".into()).unwrap_err(),
            ParseTcpShutdownError::NoSocketDescriptor.into(),
        );

        assert_eq!(
            parse_command("sd ss".into()).unwrap_err(),
            ParseTcpShutdownError::InvalidSocketDescriptor.into(),
        );

        assert_eq!(
            parse_command("sd 3 yes".into()).unwrap_err(),
            ParseTcpShutdownError::InvalidShutdownType("yes".into()).into(),
        );

        let c = parse_command("sd 3".into()).unwrap();
        assert_eq!(
            c,
            Command::Shutdown(SocketDescriptor(3), TcpShutdownKind::Write)
        );

        let c = parse_command("sd 3 r".into()).unwrap();
        assert_eq!(
            c,
            Command::Shutdown(SocketDescriptor(3), TcpShutdownKind::Read)
        );

        let c = parse_command("sd 3 read".into()).unwrap();
        assert_eq!(
            c,
            Command::Shutdown(SocketDescriptor(3), TcpShutdownKind::Read)
        );

        let c = parse_command("sd 3 w".into()).unwrap();
        assert_eq!(
            c,
            Command::Shutdown(SocketDescriptor(3), TcpShutdownKind::Write)
        );

        let c = parse_command("sd 3 write".into()).unwrap();
        assert_eq!(
            c,
            Command::Shutdown(SocketDescriptor(3), TcpShutdownKind::Write)
        );

        let c = parse_command("sd 3 both".into()).unwrap();
        assert_eq!(
            c,
            Command::Shutdown(SocketDescriptor(3), TcpShutdownKind::ReadWrite)
        );
    }

    #[test]
    fn parse_close_socket() {
        assert_eq!(
            parse_command("cl".into()).unwrap_err(),
            ParseCloseError::NoSocketDescriptor.into(),
        );

        assert_eq!(
            parse_command("cl xx".into()).unwrap_err(),
            ParseCloseError::InvalidSocketDescriptor.into(),
        );

        let c = parse_command("cl 33".into()).unwrap();
        assert_eq!(c, Command::Close(SocketDescriptor(33)));
    }

    #[test]
    fn parse_send_file() {
        assert_eq!(
            parse_command("sf".into()).unwrap_err(),
            ParseSendFileError::NoFile.into(),
        );

        assert_eq!(
            parse_command("sf hello_world".into()).unwrap_err(),
            ParseSendFileError::NoIp.into(),
        );

        assert_eq!(
            parse_command("sf hello_world 121".into()).unwrap_err(),
            ParseSendFileError::InvalidIp.into(),
        );

        assert_eq!(
            parse_command("sf hello_world 1.2.3.4".into()).unwrap_err(),
            ParseSendFileError::NoPort.into(),
        );

        assert_eq!(
            parse_command("sf hello_world 1.2.3.4 xxx".into()).unwrap_err(),
            ParseSendFileError::InvalidPort.into(),
        );

        let c = parse_command("sf hello 1.2.3.4 3434".into()).unwrap();
        assert_eq!(
            c,
            Command::SendFile {
                path: "hello".into(),
                dest_ip: Ipv4Addr::new(1, 2, 3, 4),
                port: Port(3434)
            }
        );
    }

    #[test]
    fn parse_recv_file() {
        assert_eq!(
            parse_command("rf".into()).unwrap_err(),
            ParseRecvFileError::NoFile.into()
        );

        assert_eq!(
            parse_command("rf hello".into()).unwrap_err(),
            ParseRecvFileError::NoPort.into()
        );

        assert_eq!(
            parse_command("rf hello xx".into()).unwrap_err(),
            ParseRecvFileError::InvalidPort.into()
        );

        let c = parse_command("rf hello 5000".into()).unwrap();
        assert_eq!(
            c,
            Command::RecvFile {
                out_path: "hello".into(),
                port: Port(5000)
            }
        );
    }
}
