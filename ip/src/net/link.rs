use std::net::Ipv4Addr;
use std::sync::Arc;

use etherparse::{Ipv4Header, PacketBuilder};
use tokio::{net::UdpSocket, sync::Mutex};

use crate::rip::RipMessage;

use super::utils::localhost_with_port;

pub enum ProtocolPayload {
    RIP(RipMessage),
    Test(String),
}

impl ProtocolPayload {
    fn into_bytes(self) -> (u8, Vec<u8>) {
        // TODO: handle rip and test protocol message serialization here
        match self {
            ProtocolPayload::RIP(_) => (200, Vec::new()),
            ProtocolPayload::Test(_) => (0, Vec::new()),
        }
    }
}

const TTL: u8 = 15;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct LinkDefinition {
    /// The port where the connected host runs.
    pub dest_port: u16,
    /// The virtual IP of this host's interface.
    pub interface_ip: Ipv4Addr,
    /// The virtual IP of the connected host's interface.
    pub dest_ip: Ipv4Addr,
}

pub struct Link {
    dest_port: u16,
    dest_virtual_ip: Ipv4Addr,
    src_virtual_ip: Ipv4Addr,
    sock: Arc<UdpSocket>,
}

#[derive(Debug)]
pub enum ParseLinkError {
    NoIp,
    NoPort,
    NoSrcVirtualIp,
    NoDstVirtualIp,
    MalformedPort,
    MalformedIp,
}

impl LinkDefinition {
    pub fn try_parse(raw_link: &str) -> Result<Self, ParseLinkError> {
        let mut split = raw_link.split_whitespace();

        split.next().ok_or(ParseLinkError::NoIp)?;

        let dest_port = split
            .next()
            .ok_or(ParseLinkError::NoPort)?
            .parse::<u16>()
            .map_err(|_| ParseLinkError::MalformedPort)?;

        let interface_ip = split
            .next()
            .ok_or(ParseLinkError::NoSrcVirtualIp)?
            .parse()
            .map_err(|_| ParseLinkError::MalformedIp)?;

        let dest_ip = split
            .next()
            .ok_or(ParseLinkError::NoDstVirtualIp)?
            .parse()
            .map_err(|_| ParseLinkError::MalformedIp)?;

        Ok(LinkDefinition {
            dest_port,
            interface_ip,
            dest_ip,
        })
    }

    pub fn into_link(self, udp_socket: Arc<UdpSocket>) -> Link {
        Link {
            dest_port: self.dest_port,
            dest_virtual_ip: self.dest_ip,
            src_virtual_ip: self.interface_ip,
            sock: udp_socket,
        }
    }
}

impl Link {
    /// On this link, send a message conforming to one of the supporte protocols.
    pub async fn send(&self, payload: ProtocolPayload) {
        let mut buf = Vec::new();

        let (protocol, payload) = payload.into_bytes();

        let ip_header = Ipv4Header::new(
            payload.len().try_into().expect("payload too long"),
            TTL,
            protocol,
            self.src_virtual_ip.octets(),
            self.dest_virtual_ip.octets(),
        );

        ip_header
            .write(&mut buf)
            .expect("IP header serialization error");

        buf.extend_from_slice(&payload);

        self.sock
            .send_to(&buf[..], localhost_with_port(self.dest_port))
            .await
            .unwrap();
    }
}
