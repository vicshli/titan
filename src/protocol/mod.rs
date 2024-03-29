use async_trait::async_trait;
use etherparse::Ipv4HeaderSlice;

use crate::{drop_policy::DropPolicy, net::vtlink::VtLinkNet};

pub mod rip;
pub mod tcp;
pub mod test;

#[async_trait]
pub trait ProtocolHandler<DP: DropPolicy>: Send + Sync {
    async fn handle_packet<'a>(
        &self,
        header: &Ipv4HeaderSlice<'a>,
        payload: &[u8],
        net: &VtLinkNet<DP>,
    );
}

#[derive(PartialEq, Eq, Hash, Debug)]
pub enum Protocol {
    Rip,
    Test,
    Tcp,
}

pub enum ParseProtocolError {
    Unsupported,
}

impl TryFrom<u8> for Protocol {
    type Error = ParseProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Protocol::Test),
            200 => Ok(Protocol::Rip),
            6 => Ok(Protocol::Tcp),
            _ => Err(ParseProtocolError::Unsupported),
        }
    }
}

impl TryFrom<&str> for Protocol {
    type Error = ParseProtocolError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let v = value
            .parse::<u8>()
            .map_err(|_| ParseProtocolError::Unsupported)?;
        Protocol::try_from(v)
    }
}

#[allow(clippy::from_over_into)]
impl Into<u8> for Protocol {
    fn into(self) -> u8 {
        match self {
            Protocol::Rip => 200,
            Protocol::Test => 0,
            Protocol::Tcp => 6,
        }
    }
}
